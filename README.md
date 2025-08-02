# HLS Caption Finder

A fast, efficient Rust tool for extracting closed captions from HLS (HTTP Live Streaming) video streams.

## Overview

HLS Caption Finder scans HLS playlists and extracts CEA-608/CEA-708 closed captions embedded in H.264 video streams. It processes MPEG-TS segments, locates SEI (Supplemental Enhancement Information) NALUs containing caption data, and outputs human-readable caption text.

## Features

- **Fast Processing**: Optimized bytestream processing with early termination
- **Standards Compliant**: Supports CEA-608 and CEA-708 caption standards
- **Dual Mode Support**: Handles both VOD and Live HLS streams
- **Real-time Monitoring**: Continuously monitors live streams for new captions
- **Comprehensive Parsing**: Extracts captions from H.264 SEI NALUs in MPEG-TS segments
- **Clean Output**: Filters control codes and assembles complete caption text

## Installation

### Prerequisites
- Rust 1.70+ (with Cargo)

### Build from Source
```bash
git clone https://github.com/benburkhart1/hlscaptionfinder.git
cd hlscaptionfinder
cargo build --release
```

The binary will be available at `target/release/hlscaptionfinder`.

## Usage

### Basic Usage
```bash
hlscaptionfinder <HLS_PLAYLIST_URL>
```

### Examples

**VOD Stream:**
```bash
hlscaptionfinder https://example.com/vod/master.m3u8
```

**Live Stream:**
```bash
hlscaptionfinder https://example.com/live/master.m3u8
```

### Sample Output
```
Found 95 segments to process
Processing segment 1/95: https://example.com/stream_0_000.ts
Processing segment 2/95: https://example.com/stream_0_001.ts
Processing segment 3/95: https://example.com/stream_0_002.ts
Segment: https://example.com/stream_0_002.ts
  Caption: :00,001TEST1234
Processing segment 4/95: https://example.com/stream_0_003.ts
...
Summary: 1/95 segments contained captions (1 total captions found)
```

## How It Works

1. **Playlist Analysis**: Determines if the HLS stream is VOD or Live
2. **Stream Selection**: Automatically selects the lowest bitrate variant for processing
3. **Segment Processing**: Downloads and analyzes MPEG-TS segments
4. **NALU Detection**: Locates H.264 SEI NALUs (type 6) containing caption data
5. **Caption Extraction**: Parses CEA-708 user data and decodes CEA-608 character pairs
6. **Text Assembly**: Combines character pairs into complete caption text
7. **Output**: Displays segment URLs and extracted captions

## Technical Details

### Supported Standards
- **HLS**: HTTP Live Streaming (RFC 8216)
- **MPEG-TS**: MPEG Transport Stream packets (188 bytes)
- **H.264**: Video codec with SEI NALU support
- **CEA-708**: Digital Television Closed Captioning
- **CEA-608**: Line 21 Closed Captioning (legacy)
- **ITU-T T.35**: User data format with GA94 ATSC identifier

### Stream Processing
- **VOD Mode**: Processes all segments sequentially, exits when complete
- **Live Mode**: Polls playlist at `TARGETDURATION` intervals, continues until Ctrl+C
- **Optimization**: Early termination when captions found in each segment
- **Error Handling**: Graceful handling of network errors and malformed data

### Caption Detection Pipeline
```
MPEG-TS Packet → H.264 NALU → SEI Message → CEA-708 Data → CEA-608 Characters → Caption Text
```

## Performance

The tool is optimized for speed with:
- Single-pass processing per segment
- Minimal memory allocations
- Early termination optimizations
- Efficient byte pattern matching
- Reduced debug logging in release builds

## Logging

Control log verbosity with the `RUST_LOG` environment variable:

```bash
# Error level only
RUST_LOG=error hlscaptionfinder <url>

# Info level (default)
RUST_LOG=info hlscaptionfinder <url>

# Debug level (verbose)
RUST_LOG=debug hlscaptionfinder <url>
```

## Limitations

- Only processes H.264 video streams (H.265 support planned)
- Requires captions to be embedded as SEI NALUs in the video stream
- Does not support external caption files (WebVTT, SRT, etc.)
- Selects lowest bitrate stream automatically (no manual variant selection)

## Error Handling

Common issues and solutions:

- **"Unable to determine playlist type"**: Invalid HLS URL or unreachable server
- **"No captions found"**: Stream may not contain embedded captions
- **Network timeouts**: Check internet connection and URL accessibility

## Contributing

This tool focuses on defensive security applications. Contributions for:
- Security analysis and vulnerability detection
- Performance improvements
- Additional caption format support
- Bug fixes and stability improvements

are welcome.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Built with reference to the libcaption library for CEA-608/708 parsing standards.