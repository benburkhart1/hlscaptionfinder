use std::time::Duration;
use anyhow::Result;
use clap::Parser;
use log::{info, warn, error};
use reqwest::Client;
use tokio::time::sleep;

mod hls;
mod mpeg_ts;
mod caption;
mod mpeg_bitstream;
mod cea708;
mod cea608;
mod libcaption_compat;

use hls::{HlsParser, PlaylistType};
use mpeg_ts::MpegTsParser;
use caption::CaptionDetector;
use libcaption_compat::LibcaptionTsParser;

#[derive(Parser)]
#[command(name = "hlscaptionfinder")]
#[command(about = "A CLI tool to find captions in HLS streams")]
struct Args {
    #[arg(help = "HLS playlist URL")]
    playlist_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    let args = Args::parse();
    let client = Client::new();
    
    info!("Starting HLS Caption Finder for: {}", args.playlist_url);
    
    let hls_parser = HlsParser::new(client.clone());
    let playlist_type = hls_parser.determine_playlist_type(&args.playlist_url).await?;
    
    match playlist_type {
        PlaylistType::Live { target_duration } => {
            info!("Detected live playlist with target duration: {}s", target_duration);
            process_live_playlist(&hls_parser, &args.playlist_url, target_duration).await?;
        }
        PlaylistType::Vod => {
            info!("Detected VOD playlist");
            process_vod_playlist(&hls_parser, &args.playlist_url).await?;
        }
    }
    
    Ok(())
}

async fn process_live_playlist(
    hls_parser: &HlsParser,
    playlist_url: &str,
    target_duration: u32,
) -> Result<()> {
    let mut processed_segments = std::collections::HashSet::new();
    let poll_interval = Duration::from_secs(target_duration as u64);
    
    info!("Starting live playlist polling every {}s", target_duration);
    
    loop {
        match process_current_segments(hls_parser, playlist_url, &mut processed_segments).await {
            Ok(_) => {
                info!("Completed live playlist poll cycle");
            }
            Err(e) => {
                error!("Error processing segments: {}", e);
            }
        }
        
        sleep(poll_interval).await;
    }
}

async fn process_vod_playlist(hls_parser: &HlsParser, playlist_url: &str) -> Result<()> {
    let mut processed_segments = std::collections::HashSet::new();
    
    info!("Processing all segments in VOD playlist");
    
    let segments = hls_parser.get_lowest_bitrate_segments(playlist_url).await?;
    let total_segments = segments.len();
    println!("Found {} segments to process", total_segments);
    info!("Found {} segments to process", total_segments);
    
    let mut segments_with_captions = 0;
    let mut total_captions = 0;
    
    process_current_segments_with_progress(
        hls_parser, 
        playlist_url, 
        &mut processed_segments,
        &mut segments_with_captions,
        &mut total_captions,
        total_segments
    ).await?;
    
    println!("Completed processing all segments");
    println!("Summary: {}/{} segments contained captions ({} total captions found)", 
          segments_with_captions, total_segments, total_captions);
    info!("Completed processing all segments");
    info!("Summary: {}/{} segments contained captions ({} total captions found)", 
          segments_with_captions, total_segments, total_captions);
    Ok(())
}

async fn process_current_segments(
    hls_parser: &HlsParser,
    playlist_url: &str,
    processed_segments: &mut std::collections::HashSet<String>,
) -> Result<()> {
    let segments = hls_parser.get_lowest_bitrate_segments(playlist_url).await?;
    let client = hls_parser.client();
    let mpeg_parser = MpegTsParser::new();
    let caption_detector = CaptionDetector::new();
    
    for segment in segments {
        if processed_segments.contains(&segment.uri) {
            continue;
        }
        
        info!("Processing segment: {}", segment.uri);
        
        match download_and_process_segment(
            client,
            &segment.uri,
            &mpeg_parser,
            &caption_detector,
        ).await {
            Ok(captions) => {
                if !captions.is_empty() {
                    println!("Segment: {}", segment.uri);
                    for caption in captions {
                        println!("  Caption: {}", caption);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to process segment {}: {}", segment.uri, e);
            }
        }
        
        processed_segments.insert(segment.uri);
    }
    
    Ok(())
}

async fn process_current_segments_with_progress(
    hls_parser: &HlsParser,
    playlist_url: &str,
    processed_segments: &mut std::collections::HashSet<String>,
    segments_with_captions: &mut usize,
    total_captions: &mut usize,
    total_segments: usize,
) -> Result<()> {
    let segments = hls_parser.get_lowest_bitrate_segments(playlist_url).await?;
    let client = hls_parser.client();
    let mpeg_parser = MpegTsParser::new();
    let caption_detector = CaptionDetector::new();
    
    let mut processed_count = 0;
    
    for segment in segments {
        if processed_segments.contains(&segment.uri) {
            continue;
        }
        
        processed_count += 1;
        println!("Processing segment {}/{}: {}", processed_count, total_segments, segment.uri);
        
        match download_and_process_segment(
            client,
            &segment.uri,
            &mpeg_parser,
            &caption_detector,
        ).await {
            Ok(captions) => {
                if !captions.is_empty() {
                    *segments_with_captions += 1;
                    let caption_count = captions.len();
                    *total_captions += caption_count;
                    println!("Segment: {}", segment.uri);
                    for caption in captions {
                        println!("  Caption: {}", caption);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to process segment {}: {}", segment.uri, e);
            }
        }
        
        processed_segments.insert(segment.uri);
        
        if processed_count % 10 == 0 {
            println!("Progress: {}/{} segments processed ({:.1}%)", 
                  processed_count, total_segments, 
                  (processed_count as f64 / total_segments as f64) * 100.0);
            info!("Progress: {}/{} segments processed ({:.1}%)", 
                  processed_count, total_segments, 
                  (processed_count as f64 / total_segments as f64) * 100.0);
        }
    }
    
    Ok(())
}

async fn download_and_process_segment(
    client: &Client,
    segment_url: &str,
    _mpeg_parser: &MpegTsParser,
    _caption_detector: &CaptionDetector,
) -> Result<Vec<String>> {
    let response = client.get(segment_url).send().await?;
    let segment_data = response.bytes().await?;
    // Use optimized libcaption-compatible parser
    let mut libcaption_parser = LibcaptionTsParser::new();
    let captions = libcaption_parser.parse_ts_file(&segment_data)?;
    Ok(captions)
}