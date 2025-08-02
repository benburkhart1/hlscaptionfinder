#![allow(dead_code)]
use anyhow::Result;
use log::debug;
use crate::mpeg_ts::PesPacket;

const H264_START_CODE: [u8; 4] = [0x00, 0x00, 0x00, 0x01];
const H264_START_CODE_SHORT: [u8; 3] = [0x00, 0x00, 0x01];

const NALU_TYPE_SEI: u8 = 6;
const SEI_TYPE_USER_DATA_REGISTERED: u8 = 4;
const SEI_TYPE_USER_DATA_UNREGISTERED: u8 = 5;

const CEA_608_IDENTIFIER: [u8; 4] = [0x47, 0x41, 0x39, 0x34]; // "GA94"
const CEA_708_USER_DATA_TYPE: u8 = 0x03;

pub struct CaptionDetector;

impl CaptionDetector {
    pub fn new() -> Self {
        Self
    }
    
    pub fn detect_captions(&self, pes_packets: &[PesPacket]) -> Result<Vec<String>> {
        let mut captions = Vec::new();
        
        for pes_packet in pes_packets {
            let packet_captions = self.extract_captions_from_pes(&pes_packet.data)?;
            captions.extend(packet_captions);
        }
        
        Ok(captions)
    }
    
    fn extract_captions_from_pes(&self, data: &[u8]) -> Result<Vec<String>> {
        let mut captions = Vec::new();
        
        
        let nalus = self.extract_nalus(data)?;
        
        for nalu in nalus {
            if self.is_sei_nalu(&nalu) {
                let sei_captions = self.extract_captions_from_sei(&nalu)?;
                captions.extend(sei_captions);
            }
        }
        
        Ok(captions)
    }
    
    fn extract_nalus(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        let mut nalus = Vec::new();
        let mut i = 0;
        
        while i < data.len() {
            if let Some(start_pos) = self.find_start_code(&data[i..]) {
                let actual_start = i + start_pos;
                
                if let Some(next_start) = self.find_start_code(&data[actual_start + 4..]) {
                    let next_actual_start = actual_start + 4 + next_start;
                    let nalu_data = data[actual_start + 4..next_actual_start].to_vec();
                    nalus.push(nalu_data);
                    i = next_actual_start;
                } else {
                    let nalu_data = data[actual_start + 4..].to_vec();
                    nalus.push(nalu_data);
                    break;
                }
            } else {
                break;
            }
        }
        
        Ok(nalus)
    }
    
    fn find_start_code(&self, data: &[u8]) -> Option<usize> {
        for i in 0..data.len().saturating_sub(4) {
            if data[i..i + 4] == H264_START_CODE {
                return Some(i);
            }
        }
        
        for i in 0..data.len().saturating_sub(3) {
            if data[i..i + 3] == H264_START_CODE_SHORT {
                return Some(i);
            }
        }
        
        None
    }
    
    fn is_sei_nalu(&self, nalu: &[u8]) -> bool {
        if nalu.is_empty() {
            return false;
        }
        
        let nalu_type = nalu[0] & 0x1F;
        nalu_type == NALU_TYPE_SEI
    }
    
    fn extract_captions_from_sei(&self, nalu: &[u8]) -> Result<Vec<String>> {
        let mut captions = Vec::new();
        let mut i = 1; // Skip NALU header
        
        while i < nalu.len() {
            let (sei_type, consumed) = self.read_sei_type(&nalu[i..])?;
            i += consumed;
            
            if i >= nalu.len() {
                break;
            }
            
            let (sei_size, consumed) = self.read_sei_size(&nalu[i..])?;
            i += consumed;
            
            if i + sei_size > nalu.len() {
                break;
            }
            
            if sei_type == SEI_TYPE_USER_DATA_REGISTERED {
                let sei_data = &nalu[i..i + sei_size];
                if let Ok(caption_text) = self.parse_cea_608_708_data(sei_data) {
                    if !caption_text.is_empty() {
                        captions.push(caption_text);
                    }
                }
            } else if sei_type == SEI_TYPE_USER_DATA_UNREGISTERED {
                let sei_data = &nalu[i..i + sei_size];
                // Also try parsing unregistered user data for captions
                if let Ok(caption_text) = self.parse_cea_608_708_data(sei_data) {
                    if !caption_text.is_empty() {
                        captions.push(caption_text);
                    }
                }
            } else {
                let sei_data = &nalu[i..i + sei_size];
                
                // Try parsing as CEA data anyway - some encoders use non-standard SEI types
                if let Ok(caption_text) = self.parse_cea_608_708_data(sei_data) {
                    if !caption_text.is_empty() {
                        captions.push(caption_text);
                    }
                }
                
            }
            
            i += sei_size;
        }
        
        Ok(captions)
    }
    
    fn read_sei_type(&self, data: &[u8]) -> Result<(u8, usize)> {
        let mut sei_type = 0u8;
        let mut i = 0;
        
        while i < data.len() && data[i] == 0xFF {
            sei_type = sei_type.saturating_add(255);
            i += 1;
        }
        
        if i < data.len() {
            sei_type = sei_type.saturating_add(data[i]);
            i += 1;
        }
        
        Ok((sei_type, i))
    }
    
    fn read_sei_size(&self, data: &[u8]) -> Result<(usize, usize)> {
        let mut sei_size = 0usize;
        let mut i = 0;
        
        while i < data.len() && data[i] == 0xFF {
            sei_size = sei_size.saturating_add(255);
            i += 1;
        }
        
        if i < data.len() {
            sei_size = sei_size.saturating_add(data[i] as usize);
            i += 1;
        }
        
        Ok((sei_size, i))
    }
    
    fn parse_cea_608_708_data(&self, data: &[u8]) -> Result<String> {
        if data.len() < 8 {
            debug!("CEA data too short: {} bytes", data.len());
            return Ok(String::new());
        }
        
        
        // Check for CEA-608/708 identifier
        if &data[0..4] != &CEA_608_IDENTIFIER {
            debug!("CEA identifier mismatch. Expected: {:02x?}, Got: {:02x?}", 
                   &CEA_608_IDENTIFIER, &data[0..4]);
            
            // Try alternative approach - look for caption data without GA94 identifier
            return self.parse_raw_caption_data(data);
        }
        
        let user_data_type_code = data[4];
        if user_data_type_code != CEA_708_USER_DATA_TYPE {
            debug!("User data type code mismatch");
            return Ok(String::new());
        }
        
        // Skip reserved bits and process_em_data_flag
        let mut i = 5;
        if i >= data.len() {
            return Ok(String::new());
        }
        
        let cc_count = data[i] & 0x1F;
        i += 1;
        
        let mut caption_text = String::new();
        
        for _ in 0..cc_count {
            if i + 2 >= data.len() {
                break;
            }
            
            let cc_valid = (data[i] & 0x04) != 0;
            let cc_type = data[i] & 0x03;
            
            
            if cc_valid && cc_type <= 3 { // CEA-608 data (CC1, CC2, CC3, CC4)
                let cc_data_1 = data[i + 1];
                let cc_data_2 = data[i + 2];
                
                // Basic CEA-608 character extraction (simplified)
                if let Some(text) = self.decode_cea608_chars(cc_data_1, cc_data_2) {
                    caption_text.push_str(&text);
                }
            }
            
            i += 3;
        }
        
        Ok(caption_text.trim().to_string())
    }
    
    fn decode_cea608_chars(&self, data1: u8, data2: u8) -> Option<String> {
        // This is a simplified CEA-608 decoder
        // In a full implementation, you'd need to handle control codes, 
        // special characters, and maintain state
        
        let mut result = String::new();
        
        // Check if it's printable ASCII (basic characters)
        if data1 >= 0x20 && data1 <= 0x7F {
            if let Some(ch) = char::from_u32(data1 as u32) {
                if ch.is_ascii() && !ch.is_control() {
                    result.push(ch);
                }
            }
        }
        
        if data2 >= 0x20 && data2 <= 0x7F {
            if let Some(ch) = char::from_u32(data2 as u32) {
                if ch.is_ascii() && !ch.is_control() {
                    result.push(ch);
                }
            }
        }
        
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
    
    fn parse_raw_caption_data(&self, data: &[u8]) -> Result<String> {
        let mut caption_text = String::new();
        
        // Simple approach: look for printable ASCII characters in pairs
        // This is a fallback for non-standard CEA data encoding
        for chunk in data.chunks_exact(2) {
            if chunk.len() == 2 {
                if let Some(text) = self.decode_cea608_chars(chunk[0], chunk[1]) {
                    caption_text.push_str(&text);
                }
            }
        }
        
        // Also try looking for patterns that might be encoded differently
        // Check if data contains printable ASCII text directly
        if let Ok(text) = std::str::from_utf8(data) {
            let printable: String = text.chars()
                .filter(|c| c.is_ascii() && !c.is_control())
                .collect();
            if !printable.is_empty() && printable.len() > 2 {
                caption_text.push_str(&printable);
            }
        }
        
        Ok(caption_text.trim().to_string())
    }
}