#![allow(dead_code)]
use anyhow::Result;

const TS_PACKET_SIZE: usize = 188;
const STREAM_TYPE_H264: u8 = 0x1B;
const STREAM_TYPE_H265: u8 = 0x24;

pub struct LibcaptionTsParser {
    pmtpid: Option<u16>,
    ccpid: Option<u16>,
    stream_type: Option<u8>,
    pts: Option<i64>,
    dts: Option<i64>,
    data: Vec<u8>,
}

impl LibcaptionTsParser {
    pub fn new() -> Self {
        Self {
            pmtpid: None,
            ccpid: None,
            stream_type: None,
            pts: None,
            dts: None,
            data: Vec::new(),
        }
    }
    
    pub fn parse_ts_file(&mut self, data: &[u8]) -> Result<Vec<String>> {
        let mut captions = Vec::new();
        let mut i = 0;
        let mut video_pid_found = false;
        
        // Quick scan for video stream setup
        while i + TS_PACKET_SIZE <= data.len() && (!video_pid_found || self.ccpid.is_none()) {
            let packet = &data[i..i + TS_PACKET_SIZE];
            
            if packet[0] == 0x47 { // TS sync byte
                self.parse_packet(packet)?; // This sets up PMT and video PID
                if self.ccpid.is_some() {
                    video_pid_found = true;
                }
            }
            
            i += TS_PACKET_SIZE;
        }
        
        // If no video stream found, return early
        if self.ccpid.is_none() {
            return Ok(captions);
        }
        
        // Continue processing for caption data
        while i + TS_PACKET_SIZE <= data.len() {
            let packet = &data[i..i + TS_PACKET_SIZE];
            
            if packet[0] == 0x47 { // TS sync byte
                if let Some(video_data) = self.parse_packet(packet)? {
                    // Process the video data using libcaption-style approach
                    let extracted_captions = self.process_video_data(&video_data)?;
                    captions.extend(extracted_captions);
                    
                    // Early termination if we found captions
                    if !captions.is_empty() {
                        break;
                    }
                }
            }
            
            i += TS_PACKET_SIZE;
        }
        
        Ok(captions)
    }
    
    fn parse_packet(&mut self, packet: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut i = 0;
        let pusi = (packet[i + 1] & 0x40) != 0;
        let pid = ((packet[i + 1] as u16 & 0x1F) << 8) | packet[i + 2] as u16;
        let adaptation_present = (packet[i + 3] & 0x20) != 0;
        let payload_present = (packet[i + 3] & 0x10) != 0;
        i += 4;
        
        if adaptation_present {
            let adaptation_length = packet[i] as usize;
            i += 1 + adaptation_length;
        }
        
        // PAT parsing (PID 0)
        if pid == 0 && payload_present && i < packet.len() {
            i += packet[i] as usize + 1; // Skip pointer field
            if i + 12 <= packet.len() {
                self.pmtpid = Some(((packet[i + 10] as u16 & 0x1F) << 8) | packet[i + 11] as u16);
            }
            return Ok(None);
        }
        
        // PMT parsing  
        if let Some(pmtpid) = self.pmtpid {
            if pid == pmtpid && payload_present && i < packet.len() {
                i += packet[i] as usize + 1; // Skip pointer field
                
                if i + 12 <= packet.len() {
                    let section_length = ((packet[i + 1] as u16 & 0x0F) << 8) | packet[i + 2] as u16;
                    let current = (packet[i + 5] & 0x01) != 0;
                    let program_info_length = ((packet[i + 10] as u16 & 0x0F) << 8) | packet[i + 11] as u16;
                    let mut descriptor_loop_length = section_length as i32 - (9 + program_info_length as i32 + 4);
                    
                    i += 12 + program_info_length as usize;
                    
                    if current {
                        while descriptor_loop_length >= 5 && i + 5 <= packet.len() {
                            let stream_type = packet[i];
                            let elementary_pid = ((packet[i + 1] as u16 & 0x1F) << 8) | packet[i + 2] as u16;
                            let esinfo_length = ((packet[i + 3] as u16 & 0x0F) << 8) | packet[i + 4] as u16;
                            
                            if stream_type == STREAM_TYPE_H264 || stream_type == STREAM_TYPE_H265 {
                                self.ccpid = Some(elementary_pid);
                                self.stream_type = Some(stream_type);
                            }
                            
                            i += 5 + esinfo_length as usize;
                            descriptor_loop_length -= 5 + esinfo_length as i32;
                        }
                    }
                }
                return Ok(None);
            }
        }
        
        // Video stream payload
        if let Some(ccpid) = self.ccpid {
            if payload_present && pid == ccpid {
                if pusi && i + 9 <= packet.len() {
                    // Parse PES header for timestamp
                    let has_pts = (packet[i + 7] & 0x80) != 0;
                    let has_dts = (packet[i + 7] & 0x40) != 0;
                    let header_length = packet[i + 8] as usize;
                    
                    if has_pts && i + 14 <= packet.len() {
                        self.pts = Some(self.parse_timestamp(&packet[i + 9..]));
                        if has_dts && i + 19 <= packet.len() {
                            self.dts = Some(self.parse_timestamp(&packet[i + 14..]));
                        } else {
                            self.dts = self.pts;
                        }
                    }
                    
                    i += 9 + header_length;
                }
                
                if i < packet.len() {
                    return Ok(Some(packet[i..].to_vec()));
                }
            }
        }
        
        Ok(None)
    }
    
    fn parse_timestamp(&self, data: &[u8]) -> i64 {
        if data.len() < 5 {
            return 0;
        }
        
        let mut pts = 0i64;
        pts |= (data[0] as i64 & 0x0E) << 29;
        pts |= (data[1] as i64 & 0xFF) << 22;
        pts |= (data[2] as i64 & 0xFE) << 14;
        pts |= (data[3] as i64 & 0xFF) << 7;  
        pts |= (data[4] as i64 & 0xFE) >> 1;
        pts
    }
    
    fn process_video_data(&mut self, data: &[u8]) -> Result<Vec<String>> {
        self.data.extend_from_slice(data);
        
        let mut captions = Vec::new();
        let mut _processed_nalus = 0;
        
        // Look for start codes and process NALUs exactly like libcaption
        while let Some((start_pos, start_code_len)) = self.find_start_code(&self.data) {
            if let Some((next_start, _)) = self.find_start_code(&self.data[start_pos + start_code_len..]) {
                let nalu_end = start_pos + start_code_len + next_start;
                let nalu_data = &self.data[start_pos + start_code_len..nalu_end];
                
                _processed_nalus += 1;
                
                if let Some(extracted) = self.process_nalu(nalu_data)? {
                    captions.extend(extracted);
                }
                
                self.data.drain(0..nalu_end);
            } else {
                // Check if we have a start code but no next one (final NALU)
                let remaining_size = self.data.len() - start_pos - start_code_len;
                if remaining_size > 0 {
                    let nalu_data = &self.data[start_pos + start_code_len..];
                    _processed_nalus += 1;
                    
                    if let Some(extracted) = self.process_nalu(nalu_data)? {
                        captions.extend(extracted);
                    }
                    
                    self.data.clear();
                }
                break;
            }
        }
        
        Ok(captions)
    }
    
    fn find_start_code(&self, data: &[u8]) -> Option<(usize, usize)> {
        // Look for 0x00 0x00 0x00 0x01 or 0x00 0x00 0x01
        // Return (position, start_code_length)
        const START_CODE_4: [u8; 4] = [0x00, 0x00, 0x00, 0x01];
        const START_CODE_3: [u8; 3] = [0x00, 0x00, 0x01];
        
        // Check for 4-byte start code first
        if data.len() >= 4 {
            for i in 0..=data.len() - 4 {
                if data[i..i + 4] == START_CODE_4 {
                    return Some((i, 4));
                }
            }
        }
        
        // Check for 3-byte start code
        if data.len() >= 3 {
            for i in 0..=data.len() - 3 {
                if data[i..i + 3] == START_CODE_3 {
                    return Some((i, 3));
                }
            }
        }
        
        None
    }
    
    fn process_nalu(&self, nalu: &[u8]) -> Result<Option<Vec<String>>> {
        if nalu.is_empty() {
            return Ok(None);
        }
        
        let nalu_type = nalu[0] & 0x1F;
        // Look for SEI NALU (type 6 for H.264)
        if nalu_type == 6 {
            return self.parse_sei_nalu(&nalu[1..]);
        }
        
        Ok(None)
    }
    
    fn parse_sei_nalu(&self, data: &[u8]) -> Result<Option<Vec<String>>> {
        let mut captions = Vec::new();
        let mut i = 0;
        
        // Parse SEI messages
        while i + 1 < data.len() {
            // Parse payload type (with 0xFF escaping)
            let mut payload_type = 0u32;
            while i < data.len() && data[i] == 0xFF {
                payload_type += 255;
                i += 1;
            }
            
            if i >= data.len() {
                break;
            }
            
            payload_type += data[i] as u32;
            i += 1;
            
            // Parse payload size (with 0xFF escaping)
            let mut payload_size = 0u32;
            while i < data.len() && data[i] == 0xFF {
                payload_size += 255;
                i += 1;
            }
            
            if i >= data.len() {
                break;
            }
            
            payload_size += data[i] as u32;
            i += 1;
            
            if payload_type == 4 && payload_size > 0 && i + payload_size as usize <= data.len() {
                // User data registered ITU-T T.35
                let payload_data = &data[i..i + payload_size as usize];
                
                if let Some(extracted) = self.parse_cea708_data(payload_data)? {
                    captions.extend(extracted);
                }
            }
            
            i += payload_size as usize;
        }
        
        Ok(if captions.is_empty() { None } else { Some(captions) })
    }
    
    fn parse_cea708_data(&self, data: &[u8]) -> Result<Option<Vec<String>>> {
        if data.len() < 8 {
            return Ok(None);
        }
        
        // ITU-T T.35 parsing - look for US country code (0xB5)
        let mut i = 0;
        if data[i] != 0xB5 {
            return Ok(None);
        }
        i += 1;
        
        // Skip provider_code (2 bytes) 
        i += 2;
        
        // Check for GA94 identifier
        if i + 4 > data.len() || &data[i..i + 4] != b"GA94" {
            return Ok(None);
        }
        i += 4;
        
        // User data type code should be 0x03
        if i >= data.len() || data[i] != 0x03 {
            return Ok(None);
        }
        i += 1;
        
        if i >= data.len() {
            return Ok(None);
        }
        
        let cc_count = data[i] & 0x1F;
        i += 1;
        
        // Skip em_data
        i += 1;
        
        let mut caption_text = String::new();
        
        for _cc_idx in 0..cc_count {
            if i + 3 > data.len() {
                break;
            }
            
            let cc_valid = (data[i] & 0x04) != 0;
            let cc_type = data[i] & 0x03;
            let cc_data1 = data[i + 1];
            let cc_data2 = data[i + 2];
            
            if cc_valid && cc_type <= 1 { // CEA-608 data
                if !self.is_cea608_control_code(cc_data1, cc_data2) {
                    if let Some(text) = self.decode_cea608_chars(cc_data1, cc_data2) {
                        caption_text.push_str(&text);
                    }
                }
            }
            
            i += 3;
        }
        
        // Return single caption with all accumulated text
        if caption_text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(vec![caption_text]))
        }
    }
    
    fn is_cea608_control_code(&self, data1: u8, data2: u8) -> bool {
        // CEA-608 control codes and common patterns to filter out
        
        // Null padding
        if data1 == 0x00 && data2 == 0x00 {
            return true;
        }
        
        // Control codes typically start with 0x10-0x1F
        if data1 >= 0x10 && data1 <= 0x1F {
            return true;
        }
        
        // Preamble Address Codes (PAC) - positioning and styling
        if (data1 >= 0x10 && data1 <= 0x17) || (data1 >= 0x18 && data1 <= 0x1F) {
            return true;
        }
        
        // Some common control patterns we've observed
        if (data1 == 0x2C && data2 == 0x2C) || // comma pairs
           (data1 == 0x2F && data2 == 0x2F) || // slash pairs  
           (data1 == 0x40 && data2 <= 0x7F) {  // @ followed by anything (timing codes)
            return true;
        }
        
        false
    }
    
    fn decode_cea608_chars(&self, data1: u8, data2: u8) -> Option<String> {
        let mut result = String::new();
        
        // CEA-608 character extraction with parity and control code handling
        
        // Strip parity bit from both bytes
        let char1 = data1 & 0x7F;
        let char2 = data2 & 0x7F;
        
        // Check if this is a control code pair (first byte 0x10-0x1F range after parity strip)
        if char1 >= 0x10 && char1 <= 0x1F {
            // This is likely a control code, don't extract characters
            return None;
        }
        
        // Extract printable characters
        if char1 >= 0x20 && char1 <= 0x7F {
            result.push(char1 as char);
        }
        
        if char2 >= 0x20 && char2 <= 0x7F {
            result.push(char2 as char);
        }
        
        // Filter out common control characters that slip through
        let result_str = result.trim();
        if result_str.is_empty() || 
           result_str == "@" || 
           result_str == "," || 
           result_str == "/" ||
           result_str == " " {
            None
        } else {
            Some(result.to_string())
        }
    }
}