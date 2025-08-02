use anyhow::{Result, anyhow};
use reqwest::Client;
use url::Url;
use log::{debug, info};

#[derive(Debug, Clone)]
pub enum PlaylistType {
    Live { target_duration: u32 },
    Vod,
}

#[derive(Debug, Clone)]
pub struct MediaPlaylist {
    pub uri: String,
    pub bandwidth: u32,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub uri: String,
    #[allow(dead_code)]
    pub duration: f64,
}

pub struct HlsParser {
    client: Client,
}

impl HlsParser {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
    
    pub fn client(&self) -> &Client {
        &self.client
    }
    
    pub async fn determine_playlist_type(&self, playlist_url: &str) -> Result<PlaylistType> {
        let content = self.fetch_playlist_content(playlist_url).await?;
        
        if content.contains("#EXT-X-PLAYLIST-TYPE:VOD") {
            return Ok(PlaylistType::Vod);
        }
        
        if content.contains("#EXT-X-ENDLIST") {
            return Ok(PlaylistType::Vod);
        }
        
        if let Some(target_duration) = self.extract_target_duration(&content) {
            return Ok(PlaylistType::Live { target_duration });
        }
        
        if self.is_master_playlist(&content) {
            let media_playlists = self.parse_master_playlist(&content, playlist_url)?;
            if let Some(lowest_bitrate) = media_playlists.first() {
                return Box::pin(self.determine_playlist_type(&lowest_bitrate.uri)).await;
            }
        }
        
        Err(anyhow!("Unable to determine playlist type"))
    }
    
    pub async fn get_lowest_bitrate_segments(&self, playlist_url: &str) -> Result<Vec<Segment>> {
        let content = self.fetch_playlist_content(playlist_url).await?;
        
        if self.is_master_playlist(&content) {
            let media_playlists = self.parse_master_playlist(&content, playlist_url)?;
            if let Some(lowest_bitrate) = media_playlists.first() {
                return Box::pin(self.get_lowest_bitrate_segments(&lowest_bitrate.uri)).await;
            } else {
                return Err(anyhow!("No media playlists found in master playlist"));
            }
        }
        
        self.parse_media_playlist(&content, playlist_url)
    }
    
    async fn fetch_playlist_content(&self, url: &str) -> Result<String> {
        debug!("Fetching playlist: {}", url);
        let response = self.client.get(url).send().await?;
        let content = response.text().await?;
        Ok(content)
    }
    
    fn is_master_playlist(&self, content: &str) -> bool {
        content.contains("#EXT-X-STREAM-INF:")
    }
    
    fn extract_target_duration(&self, content: &str) -> Option<u32> {
        for line in content.lines() {
            if line.starts_with("#EXT-X-TARGETDURATION:") {
                if let Some(duration_str) = line.split(':').nth(1) {
                    if let Ok(duration) = duration_str.parse::<u32>() {
                        return Some(duration);
                    }
                }
            }
        }
        None
    }
    
    fn parse_master_playlist(&self, content: &str, base_url: &str) -> Result<Vec<MediaPlaylist>> {
        let mut playlists = Vec::new();
        let mut current_bandwidth = None;
        
        for line in content.lines() {
            let line = line.trim();
            
            if line.starts_with("#EXT-X-STREAM-INF:") {
                current_bandwidth = self.extract_bandwidth(line);
            } else if !line.starts_with('#') && !line.is_empty() {
                if let Some(bandwidth) = current_bandwidth {
                    let uri = self.resolve_url(base_url, line)?;
                    playlists.push(MediaPlaylist { uri, bandwidth });
                    current_bandwidth = None;
                }
            }
            // Skip empty lines - they don't reset the current_bandwidth
        }
        
        playlists.sort_by_key(|p| p.bandwidth);
        info!("Found {} media playlists, lowest bitrate: {}", playlists.len(), 
              playlists.first().map(|p| p.bandwidth).unwrap_or(0));
        
        Ok(playlists)
    }
    
    fn extract_bandwidth(&self, line: &str) -> Option<u32> {
        for attribute in line.split(',') {
            let attribute = attribute.trim();
            // Handle both "BANDWIDTH=value" and "#EXT-X-STREAM-INF:BANDWIDTH=value"
            if let Some(bandwidth_str) = attribute.strip_prefix("BANDWIDTH=") {
                if let Ok(bandwidth) = bandwidth_str.parse::<u32>() {
                    return Some(bandwidth);
                }
            } else if let Some(rest) = attribute.strip_prefix("#EXT-X-STREAM-INF:") {
                if let Some(bandwidth_str) = rest.strip_prefix("BANDWIDTH=") {
                    if let Ok(bandwidth) = bandwidth_str.parse::<u32>() {
                        return Some(bandwidth);
                    }
                }
            }
        }
        None
    }
    
    fn parse_media_playlist(&self, content: &str, base_url: &str) -> Result<Vec<Segment>> {
        let mut segments = Vec::new();
        let mut current_duration = 0.0;
        
        for line in content.lines() {
            let line = line.trim();
            
            if line.starts_with("#EXTINF:") {
                current_duration = self.extract_duration(line);
            } else if !line.starts_with('#') && !line.is_empty() {
                let uri = self.resolve_url(base_url, line)?;
                segments.push(Segment {
                    uri,
                    duration: current_duration,
                });
                current_duration = 0.0;
            }
        }
        
        info!("Found {} segments in media playlist", segments.len());
        Ok(segments)
    }
    
    fn extract_duration(&self, line: &str) -> f64 {
        if let Some(duration_part) = line.strip_prefix("#EXTINF:") {
            if let Some(duration_str) = duration_part.split(',').next() {
                return duration_str.parse::<f64>().unwrap_or(0.0);
            }
        }
        0.0
    }
    
    fn resolve_url(&self, base_url: &str, relative_url: &str) -> Result<String> {
        if relative_url.starts_with("http://") || relative_url.starts_with("https://") {
            return Ok(relative_url.to_string());
        }
        
        let base = Url::parse(base_url)?;
        let resolved = base.join(relative_url)?;
        Ok(resolved.to_string())
    }
}