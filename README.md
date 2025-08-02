HLS Caption Finder
==================

This Rust application takes a remote HLS playlist as an argument.

If the playlist is a HLS live playlist, it will poll the lowest bitrate media playlist at a duration appropriate for the TARGETDURATION, and walk through each segment, scanning the MPEG-TS segment for captions in the x264 SEI NALU,
if it detects a caption, it will output the segment name and captions detected in the segment. The user will exit hitting control-c.

If the playlist is a HLS VOD playlist, it will read every segment from the lowest bitrate media playlist, scanning the MPEG-ts segments for captions in the x264 SEI NALU, If it detects a caption, it will output the segment name,
and the captions detected in the segment. It will exit on completion of scanning these segments.
