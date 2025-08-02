use anyhow::Result;
use log::debug;

const TS_PACKET_SIZE: usize = 188;
const TS_SYNC_BYTE: u8 = 0x47;

// Stream types from libcaption
const STREAM_TYPE_H264: u8 = 0x1B;
const STREAM_TYPE_H265: u8 = 0x24;

pub enum TsParseResult {
    Ok,
    Ready { data: Vec<u8>, dts: f64, cts: f64 },
}

#[derive(Debug, Default)]
pub struct TsParser {
    pmtpid: Option<u16>,
    ccpid: Option<u16>,
    stream_type: Option<u8>,
    pts: Option<i64>,
    dts: Option<i64>,
}

impl TsParser {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn parse_packet(&mut self, packet: &[u8]) -> Result<TsParseResult> {
        if packet.len() != TS_PACKET_SIZE || packet[0] != TS_SYNC_BYTE {
            return Ok(TsParseResult::Ok);
        }
        
        let mut i = 0;
        let pusi = (packet[i + 1] & 0x40) != 0; // Payload Unit Start Indicator
        let pid = ((packet[i + 1] as u16 & 0x1F) << 8) | packet[i + 2] as u16;
        let adaptation_present = (packet[i + 3] & 0x20) != 0;
        let payload_present = (packet[i + 3] & 0x10) != 0;
        i += 4;
        
        if adaptation_present {
            let adaptation_length = packet[i] as usize;
            i += 1 + adaptation_length;
        }
        
        // PAT (Program Association Table) - PID 0
        if pid == 0 {
            if payload_present && i < packet.len() {
                // Skip the pointer field
                i += packet[i] as usize + 1;
                
                if i + 12 <= packet.len() {
                    self.pmtpid = Some(((packet[i + 10] as u16 & 0x1F) << 8) | packet[i + 11] as u16);
                    debug!("Found PMT PID: {:?}", self.pmtpid);
                }
            }
            return Ok(TsParseResult::Ok);
        }
        
        // PMT (Program Map Table)
        if let Some(pmtpid) = self.pmtpid {
            if pid == pmtpid {
                if payload_present && i < packet.len() {
                    // Skip pointer field
                    i += packet[i] as usize + 1;
                    
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
                                    debug!("Found video stream PID: {}, type: 0x{:02x}", elementary_pid, stream_type);
                                }
                                
                                i += 5 + esinfo_length as usize;
                                descriptor_loop_length -= 5 + esinfo_length as i32;
                            }
                        }
                    }
                }
                return Ok(TsParseResult::Ok);
            }
        }
        
        // Video stream payload
        if let Some(ccpid) = self.ccpid {
            if payload_present && pid == ccpid {
                if pusi {
                    // PES header parsing
                    if i + 9 <= packet.len() {
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
                }
                
                if i < packet.len() {
                    let data = packet[i..].to_vec();
                    let dts = self.dts.unwrap_or(0) as f64 / 90000.0;
                    let cts = (self.pts.unwrap_or(0) - self.dts.unwrap_or(0)) as f64 / 90000.0;
                    
                    return Ok(TsParseResult::Ready { data, dts, cts });
                }
            }
        }
        
        Ok(TsParseResult::Ok)
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
    
    pub fn stream_type(&self) -> Option<u8> {
        self.stream_type
    }
}

// Legacy compatibility - simplified interface
#[derive(Debug, Clone)]
pub struct PesPacket {
    pub data: Vec<u8>,
}

pub struct MpegTsParser;

impl MpegTsParser {
    pub fn new() -> Self {
        Self
    }
    
    pub fn extract_pes_packets(&self, data: &[u8]) -> Result<Vec<PesPacket>> {
        let mut ts_parser = TsParser::new();
        let mut packets = Vec::new();
        
        let mut i = 0;
        while i + TS_PACKET_SIZE <= data.len() {
            let packet = &data[i..i + TS_PACKET_SIZE];
            
            match ts_parser.parse_packet(packet)? {
                TsParseResult::Ready { data, .. } => {
                    packets.push(PesPacket { data });
                }
                TsParseResult::Ok => {}
            }
            
            i += TS_PACKET_SIZE;
        }
        
        debug!("Extracted {} video data packets", packets.len());
        Ok(packets)
    }
}