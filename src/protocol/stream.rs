use std::collections::HashMap;
use tracing::debug;

#[derive(Debug, Clone, PartialEq)]
pub enum StreamState {
    Closed,
    Opening,
    Open,
    Closing,
}

#[derive(Debug)]
pub struct Stream {
    pub local_id: u32,
    pub remote_id: u32,
    pub state: StreamState,
    pub service: Option<String>,
}

impl Stream {
    pub fn new(local_id: u32, remote_id: u32) -> Self {
        Self {
            local_id,
            remote_id,
            state: StreamState::Opening,
            service: None,
        }
    }

    pub fn set_service(&mut self, service: String) {
        self.service = Some(service);
    }

    pub fn open(&mut self) {
        if self.state == StreamState::Opening {
            self.state = StreamState::Open;
            debug!("Stream {}:{} opened", self.local_id, self.remote_id);
        }
    }

    pub fn close(&mut self) {
        self.state = StreamState::Closing;
        debug!("Stream {}:{} closing", self.local_id, self.remote_id);
    }
}

#[derive(Debug)]
pub struct StreamManager {
    streams: HashMap<u32, Stream>,
    next_local_id: u32,
}

impl StreamManager {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            next_local_id: 1,
        }
    }

    pub fn create_stream(&mut self, remote_id: u32) -> u32 {
        let local_id = self.next_local_id;
        self.next_local_id += 1;

        let stream = Stream::new(local_id, remote_id);
        self.streams.insert(local_id, stream);

        debug!(
            "Created stream with local_id: {}, remote_id: {}",
            local_id, remote_id
        );
        local_id
    }

    pub fn get_stream_mut(&mut self, local_id: u32) -> Option<&mut Stream> {
        self.streams.get_mut(&local_id)
    }

    pub fn get_stream(&self, local_id: u32) -> Option<&Stream> {
        self.streams.get(&local_id)
    }

    pub fn remove_stream(&mut self, local_id: u32) -> Option<Stream> {
        let stream = self.streams.remove(&local_id);
        if stream.is_some() {
            debug!("Removed stream with local_id: {}", local_id);
        }
        stream
    }

    pub fn find_stream_by_remote_id(&self, remote_id: u32) -> Option<&Stream> {
        self.streams.values().find(|s| s.remote_id == remote_id)
    }

    pub fn close_all_streams(&mut self) {
        for stream in self.streams.values_mut() {
            stream.close();
        }
        debug!("Closing all streams");
    }

    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }
}

impl Default for StreamManager {
    fn default() -> Self {
        Self::new()
    }
}
