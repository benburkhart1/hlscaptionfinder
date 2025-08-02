use anyhow::Result;
use log::debug;

const TS_PACKET_SIZE: usize = 188;
const TS_SYNC_BYTE: u8 = 0x47;
const STREAM_TYPE_H264: u8 = 0x1B;
const STREAM_TYPE_H265: u8 = 0x24;
const PAT_PID: u16 = 0x0000;

pub struct OptimizedTsParser {
    pmt_pid: Option<u16>,
    video_pid: Option<u16>,
    stream_type: Option<u8>,
    video_data_buffer: Vec<u8>,
    pat_pmt_parsed: bool,
}

impl OptimizedTsParser {
    pub fn new() -> Self {
        Self {
            pmt_pid: None,
            video_pid: None,
            stream_type: None,
            video_data_buffer: Vec::new(),
            pat_pmt_parsed: false,
        }
    }

    pub fn parse_ts_file(&mut self, data: &[u8]) -> Result<Vec<String>> {
        let mut captions = Vec::new();
        let packet_count = data.len() / TS_PACKET_SIZE;
        
        // Step 1: Split into 188-byte packets and find video PID
        for packet_idx in 0..packet_count {
            let packet_start = packet_idx * TS_PACKET_SIZE;
            let packet_end = packet_start + TS_PACKET_SIZE;
            
            if packet_end > data.len() {
                break;
            }
            
            let packet = &data[packet_start..packet_end];
            
            // Verify sync byte
            if packet[0] != TS_SYNC_BYTE {
                continue;
            }
            
            let pid = self.extract_pid(packet);
            
            // Step 2: Process PAT to find PMT PID (only if not already parsed)
            if !self.pat_pmt_parsed && pid == PAT_PID {
                self.parse_pat_packet(packet)?;
                continue;
            }
            
            // Step 3: Process PMT to find video stream PID (only if not already parsed)
            if !self.pat_pmt_parsed {
                if let Some(pmt_pid) = self.pmt_pid {
                    if pid == pmt_pid {
                        self.parse_pmt_packet(packet)?;
                        if self.video_pid.is_some() {
                            self.pat_pmt_parsed = true;
                        }
                        continue;
                    }
                }
            }
            
            // Step 4: Only process packets for the video stream's PID
            if let Some(video_pid) = self.video_pid {
                if pid == video_pid {
                    if let Some(video_data) = self.extract_video_payload(packet)? {
                        self.video_data_buffer.extend_from_slice(&video_data);
                        
                        // Process accumulated video data for NALU type 6
                        let extracted_captions = self.process_video_buffer_for_sei()?;
                        captions.extend(extracted_captions);
                        
                        // Early exit if we found captions
                        if !captions.is_empty() {
                            break;
                        }
                    }
                }
            }
        }
        
        // Process any remaining video data
        if !self.video_data_buffer.is_empty() {
            let remaining_captions = self.process_video_buffer_for_sei()?;
            captions.extend(remaining_captions);
        }
        
        Ok(captions)
    }

    fn extract_pid(&self, packet: &[u8]) -> u16 {
        ((packet[1] as u16 & 0x1F) << 8) | packet[2] as u16
    }

    fn parse_pat_packet(&mut self, packet: &[u8]) -> Result<()> {
        let payload_present = (packet[3] & 0x10) != 0;
        if !payload_present {
            return Ok(());
        }
        
        let mut i = 4;
        
        // Skip adaptation field if present
        if (packet[3] & 0x20) != 0 {
            let adaptation_length = packet[i] as usize;
            i += 1 + adaptation_length;
        }
        
        if i >= packet.len() {
            return Ok(());
        }
        
        // Skip pointer field
        i += packet[i] as usize + 1;
        
        // Extract PMT PID from PAT
        if i + 12 <= packet.len() {
            self.pmt_pid = Some(((packet[i + 10] as u16 & 0x1F) << 8) | packet[i + 11] as u16);
            debug!("Found PMT PID: {:?}", self.pmt_pid);
        }
        
        Ok(())
    }

    fn parse_pmt_packet(&mut self, packet: &[u8]) -> Result<()> {
        let payload_present = (packet[3] & 0x10) != 0;
        if !payload_present {
            return Ok(());
        }
        
        let mut i = 4;
        
        // Skip adaptation field if present
        if (packet[3] & 0x20) != 0 {
            let adaptation_length = packet[i] as usize;
            i += 1 + adaptation_length;
        }
        
        if i >= packet.len() {
            return Ok(());
        }
        
        // Skip pointer field
        i += packet[i] as usize + 1;
        
        if i + 12 > packet.len() {
            return Ok(());
        }
        
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
                
                // Find video stream
                if stream_type == STREAM_TYPE_H264 || stream_type == STREAM_TYPE_H265 {
                    self.video_pid = Some(elementary_pid);
                    self.stream_type = Some(stream_type);
                    debug!("Found video stream PID: {}, type: 0x{:02x}", elementary_pid, stream_type);
                }
                
                i += 5 + esinfo_length as usize;
                descriptor_loop_length -= 5 + esinfo_length as i32;
            }
        }
        
        Ok(())
    }

    fn extract_video_payload(&self, packet: &[u8]) -> Result<Option<Vec<u8>>> {
        let pusi = (packet[1] & 0x40) != 0;
        let payload_present = (packet[3] & 0x10) != 0;
        
        if !payload_present {
            return Ok(None);
        }
        
        let mut i = 4;
        
        // Skip adaptation field if present
        if (packet[3] & 0x20) != 0 {
            let adaptation_length = packet[i] as usize;
            i += 1 + adaptation_length;
        }
        
        // Skip PES header if this is the start of a new PES packet
        if pusi && i + 9 <= packet.len() {
            let header_length = packet[i + 8] as usize;
            i += 9 + header_length;
        }
        
        if i < packet.len() {
            Ok(Some(packet[i..].to_vec()))
        } else {
            Ok(None)
        }
    }

    fn process_video_buffer_for_sei(&mut self) -> Result<Vec<String>> {
        let mut captions = Vec::new();
        let mut processed_bytes = 0;
        
        // Look for NALU start codes and process only NALU type 6 (SEI)
        while let Some((start_pos, start_code_len)) = self.find_start_code(&self.video_data_buffer[processed_bytes..]) {
            let absolute_start = processed_bytes + start_pos;
            let nalu_header_pos = absolute_start + start_code_len;
            
            if let Some((next_start, _)) = self.find_start_code(&self.video_data_buffer[nalu_header_pos..]) {
                let nalu_end = nalu_header_pos + next_start;
                
                if nalu_header_pos < self.video_data_buffer.len() {
                    let nalu_data = &self.video_data_buffer[nalu_header_pos..nalu_end.min(self.video_data_buffer.len())];
                    
                    // Step 5: Check NALU type and skip if not type 6
                    if !nalu_data.is_empty() {
                        let nalu_type = nalu_data[0] & 0x1F;
                        if nalu_type == 6 {
                            // Only process SEI NALUs (type 6)
                            if let Some(extracted) = self.process_sei_nalu(&nalu_data[1..])? {
                                captions.extend(extracted);
                            }
                        }
                    }
                }
                
                processed_bytes = nalu_end;
            } else {
                // Process final NALU if present
                if nalu_header_pos < self.video_data_buffer.len() {
                    let nalu_data = &self.video_data_buffer[nalu_header_pos..];
                    
                    if !nalu_data.is_empty() {
                        let nalu_type = nalu_data[0] & 0x1F;
                        if nalu_type == 6 {
                            if let Some(extracted) = self.process_sei_nalu(&nalu_data[1..])? {
                                captions.extend(extracted);
                            }
                        }
                    }
                }
                break;
            }
        }
        
        // Only clear buffer if we processed everything or found captions
        if processed_bytes > 0 || !captions.is_empty() {
            if processed_bytes >= self.video_data_buffer.len() {
                self.video_data_buffer.clear();
            } else {
                self.video_data_buffer.drain(0..processed_bytes);
            }
        }
        
        Ok(captions)
    }

    fn find_start_code(&self, data: &[u8]) -> Option<(usize, usize)> {
        // Use optimized scanning - look for 0x00 first, then verify pattern
        let mut i = 0;
        while i + 2 < data.len() {
            // Quick scan for 0x00 0x00 pattern
            if data[i] == 0x00 && data[i + 1] == 0x00 {
                // Check for 4-byte start code
                if i + 3 < data.len() && data[i + 2] == 0x00 && data[i + 3] == 0x01 {
                    return Some((i, 4));
                }
                // Check for 3-byte start code
                if data[i + 2] == 0x01 {
                    return Some((i, 3));
                }
                i += 2; // Skip past this 0x00 0x00 sequence
            } else {
                i += 1;
            }
        }
        None
    }

    fn process_sei_nalu(&self, data: &[u8]) -> Result<Option<Vec<String>>> {
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
            
            if cc_valid && cc_type <= 1 {
                if !self.is_cea608_control_code(cc_data1, cc_data2) {
                    if let Some(text) = self.decode_cea608_chars(cc_data1, cc_data2) {
                        caption_text.push_str(&text);
                    }
                }
            }
            
            i += 3;
        }
        
        if caption_text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(vec![caption_text]))
        }
    }

    fn is_cea608_control_code(&self, data1: u8, data2: u8) -> bool {
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
        
        // Some common control patterns
        if (data1 == 0x2C && data2 == 0x2C) || 
           (data1 == 0x2F && data2 == 0x2F) || 
           (data1 == 0x40 && data2 <= 0x7F) {
            return true;
        }
        
        false
    }

    fn decode_cea608_chars(&self, data1: u8, data2: u8) -> Option<String> {
        let mut result = String::new();
        
        // Strip parity bit from both bytes
        let char1 = data1 & 0x7F;
        let char2 = data2 & 0x7F;
        
        // Check if this is a control code pair
        if char1 >= 0x10 && char1 <= 0x1F {
            return None;
        }
        
        // Extract printable characters
        if char1 >= 0x20 && char1 <= 0x7F {
            result.push(char1 as char);
        }
        
        if char2 >= 0x20 && char2 <= 0x7F {
            result.push(char2 as char);
        }
        
        // Filter out common control characters
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