use anyhow::Result;
use log::debug;

const MAX_NALU_SIZE: usize = 6 * 1024 * 1024;
const H264_START_CODE: [u8; 4] = [0x00, 0x00, 0x00, 0x01];
const H264_START_CODE_SHORT: [u8; 3] = [0x00, 0x00, 0x01];

// NALU types
const H264_SEI_PACKET: u8 = 0x06;
const H265_SEI_PACKET: u8 = 0x27;

// Stream types
const STREAM_TYPE_H264: u8 = 0x1B;
const STREAM_TYPE_H265: u8 = 0x24;

pub struct MpegBitstream {
    buffer: Vec<u8>,
    stream_type: Option<u8>,
}

impl MpegBitstream {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            stream_type: None,
        }
    }
    
    pub fn set_stream_type(&mut self, stream_type: u8) {
        self.stream_type = Some(stream_type);
    }
    
    pub fn parse(&mut self, data: &[u8], dts: f64, cts: f64) -> Result<Vec<SeiMessage>> {
        if self.buffer.len() + data.len() > MAX_NALU_SIZE {
            // Buffer too large, start fresh
            self.buffer.clear();
        }
        
        // Debug: Check if the raw data contains TEST1234
        if data.len() >= 8 {
            let test_bytes = b"TEST1234";
            if data.windows(test_bytes.len()).any(|window| window == test_bytes) {
                debug!("Found TEST1234 bytes in raw video data before NALU parsing!");
            }
        }
        
        self.buffer.extend_from_slice(data);
        
        let mut sei_messages = Vec::new();
        
        while let Some(start_pos) = self.find_start_code(&self.buffer) {
            if let Some(next_start) = self.find_start_code(&self.buffer[start_pos + 4..]) {
                let nalu_end = start_pos + 4 + next_start;
                let nalu_data = &self.buffer[start_pos + 4..nalu_end];
                
                if let Some(messages) = self.process_nalu(nalu_data, dts + cts)? {
                    sei_messages.extend(messages);
                }
                
                // Remove processed data
                self.buffer.drain(0..nalu_end);
            } else {
                // Incomplete NALU at end of buffer
                break;
            }
        }
        
        Ok(sei_messages)
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
    
    fn process_nalu(&self, nalu: &[u8], timestamp: f64) -> Result<Option<Vec<SeiMessage>>> {
        if nalu.is_empty() {
            return Ok(None);
        }
        
        let nalu_type = nalu[0] & 0x1F;
        debug!("Processing NALU type: {}, size: {} bytes", nalu_type, nalu.len());
        
        let is_sei = match self.stream_type {
            Some(STREAM_TYPE_H264) => nalu_type == H264_SEI_PACKET,
            Some(STREAM_TYPE_H265) => nalu_type == H265_SEI_PACKET,
            _ => false,
        };
        
        if is_sei {
            debug!("Found SEI NALU with {} bytes", nalu.len());
            return self.parse_sei(&nalu[1..], timestamp);
        } else if nalu_type == H264_SEI_PACKET {
            debug!("Found NALU type 6 (SEI) but stream type check failed. Stream type: {:?}", self.stream_type);
        }
        
        Ok(None)
    }
    
    fn parse_sei(&self, data: &[u8], timestamp: f64) -> Result<Option<Vec<SeiMessage>>> {
        let mut messages = Vec::new();
        let mut i = 0;
        
        while i + 1 < data.len() {
            // Parse payload type
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
            
            // Parse payload size
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
            
            if payload_size > 0 && i + payload_size as usize <= data.len() {
                let payload_data = &data[i..i + payload_size as usize];
                
                // Apply RBSP de-emulation
                let rbsp_data = self.remove_emulation_prevention_bytes(payload_data);
                
                messages.push(SeiMessage {
                    payload_type,
                    data: rbsp_data,
                    timestamp,
                });
                
                debug!("Found SEI message type {} with {} bytes", payload_type, payload_size);
                
                i += payload_size as usize;
            } else {
                break;
            }
        }
        
        Ok(if messages.is_empty() { None } else { Some(messages) })
    }
    
    fn remove_emulation_prevention_bytes(&self, data: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        let mut i = 0;
        
        while i < data.len() {
            if i + 2 < data.len() && data[i] == 0x00 && data[i + 1] == 0x00 && data[i + 2] == 0x03 {
                // Found emulation prevention pattern: 0x00 0x00 0x03
                result.push(0x00);
                result.push(0x00);
                i += 3; // Skip the 0x03
            } else {
                result.push(data[i]);
                i += 1;
            }
        }
        
        result
    }
    
    pub fn flush(&mut self) -> Vec<SeiMessage> {
        // Process any remaining data in buffer
        if let Some(start_pos) = self.find_start_code(&self.buffer) {
            let nalu_data = &self.buffer[start_pos + 4..];
            if let Ok(Some(messages)) = self.process_nalu(nalu_data, 0.0) {
                self.buffer.clear();
                return messages;
            }
        }
        
        self.buffer.clear();
        Vec::new()
    }
}

#[derive(Debug, Clone)]
pub struct SeiMessage {
    pub payload_type: u32,
    pub data: Vec<u8>,
    pub timestamp: f64,
}

impl SeiMessage {
    pub fn is_user_data_registered(&self) -> bool {
        self.payload_type == 4 // sei_type_user_data_registered_itu_t_t35
    }
}