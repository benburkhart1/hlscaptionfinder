#![allow(dead_code)]
use anyhow::Result;
use log::debug;

// ITU-T T.35 country codes
const T35_COUNTRY_CODE_US: u8 = 0xB5;

// ATSC A/53 constants
const ATSC_IDENTIFIER: [u8; 4] = [0x47, 0x41, 0x39, 0x34]; // "GA94"
const USER_DATA_TYPE_CODE: u8 = 0x03;

#[derive(Debug, Clone)]
pub struct CaptionData {
    pub cc_valid: bool,
    pub cc_type: u8,
    pub cc_data: [u8; 2],
}

pub struct Cea708Parser;

impl Cea708Parser {
    pub fn new() -> Self {
        Self
    }
    
    pub fn parse_user_data(&self, data: &[u8]) -> Result<Vec<CaptionData>> {
        if data.len() < 8 {
            debug!("User data too short: {} bytes", data.len());
            return Ok(Vec::new());
        }
        
        debug!("Parsing user data, length: {}, first 8 bytes: {:02x?}", data.len(), &data[0..8.min(data.len())]);
        
        let mut i = 0;
        
        // Check for ITU-T T.35 country code
        if data[i] != T35_COUNTRY_CODE_US {
            debug!("Not US country code: 0x{:02x}", data[i]);
            return Ok(Vec::new());
        }
        i += 1;
        
        // Skip provider_code (2 bytes) and user_identifier (4 bytes)
        if i + 6 > data.len() {
            debug!("Not enough data for provider code and identifier");
            return Ok(Vec::new());
        }
        
        // Check for ATSC A/53 identifier "GA94"
        if &data[i..i + 4] != &ATSC_IDENTIFIER {
            debug!("ATSC identifier mismatch. Expected: {:02x?}, Got: {:02x?}", 
                   &ATSC_IDENTIFIER, &data[i..i + 4]);
            return Ok(Vec::new());
        }
        i += 4;
        
        // Check user_data_type_code
        if data[i] != USER_DATA_TYPE_CODE {
            debug!("User data type code mismatch. Expected: 0x{:02x}, Got: 0x{:02x}", 
                   USER_DATA_TYPE_CODE, data[i]);
            return Ok(Vec::new());
        }
        i += 1;
        
        if i >= data.len() {
            debug!("No data after user_data_type_code");
            return Ok(Vec::new());
        }
        
        // Parse process_em_data_flag and process_cc_data_flag
        let process_em_data_flag = (data[i] & 0x80) != 0;
        let process_cc_data_flag = (data[i] & 0x40) != 0;
        let additional_data_flag = (data[i] & 0x20) != 0;
        let cc_count = data[i] & 0x1F;
        i += 1;
        
        debug!("process_em_data_flag: {}, process_cc_data_flag: {}, additional_data_flag: {}, cc_count: {}", 
               process_em_data_flag, process_cc_data_flag, additional_data_flag, cc_count);
        
        if !process_cc_data_flag {
            debug!("process_cc_data_flag is false, no caption data");
            return Ok(Vec::new());
        }
        
        let mut captions = Vec::new();
        
        // Reserved byte
        if i >= data.len() {
            debug!("Missing reserved byte");
            return Ok(Vec::new());
        }
        i += 1; // Skip em_data
        
        // Parse cc_data structures
        for cc_index in 0..cc_count {
            if i + 3 > data.len() {
                debug!("Not enough data for cc_data {}", cc_index);
                break;
            }
            
            let marker_bits = (data[i] & 0xF8) >> 3;
            let cc_valid = (data[i] & 0x04) != 0;
            let cc_type = data[i] & 0x03;
            let cc_data = [data[i + 1], data[i + 2]];
            
            debug!("CC {}: marker_bits=0x{:02x}, cc_valid={}, cc_type={}, cc_data=[0x{:02x}, 0x{:02x}]", 
                   cc_index, marker_bits, cc_valid, cc_type, cc_data[0], cc_data[1]);
            
            if marker_bits != 0x1F {
                debug!("Invalid marker bits: expected 0x1F, got 0x{:02x}", marker_bits);
            }
            
            if cc_valid {
                captions.push(CaptionData {
                    cc_valid,
                    cc_type,
                    cc_data,
                });
            }
            
            i += 3;
        }
        
        debug!("Extracted {} valid caption data entries", captions.len());
        
        Ok(captions)
    }
}