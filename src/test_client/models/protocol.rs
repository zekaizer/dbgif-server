use crate::protocol::Message;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ProtocolExchange {
    pub request: Message,
    pub response: Option<Message>,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
    pub success: bool,
    pub error: Option<String>,
    pub context: String,
}

impl ProtocolExchange {
    pub fn new(request: Message, context: impl Into<String>) -> Self {
        Self {
            request,
            response: None,
            start_time: Instant::now(),
            end_time: None,
            success: false,
            error: None,
            context: context.into(),
        }
    }

    pub fn complete_with_response(&mut self, response: Message) {
        self.response = Some(response);
        self.end_time = Some(Instant::now());
        self.success = true;
        self.error = None;
    }

    pub fn complete_with_error(&mut self, error: impl Into<String>) {
        self.end_time = Some(Instant::now());
        self.success = false;
        self.error = Some(error.into());
    }

    pub fn complete_with_timeout(&mut self) {
        self.end_time = Some(Instant::now());
        self.success = false;
        self.error = Some("Exchange timed out".to_string());
    }

    pub fn duration(&self) -> Duration {
        match self.end_time {
            Some(end) => end.duration_since(self.start_time),
            None => self.start_time.elapsed(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.end_time.is_some()
    }

    pub fn is_successful(&self) -> bool {
        self.success && self.response.is_some()
    }

    pub fn summary(&self) -> String {
        let status = if self.success { "SUCCESS" } else { "FAILED" };
        let duration_ms = self.duration().as_millis();
        let context = &self.context;

        match &self.error {
            Some(err) => format!("{} ({}) in {}ms: {}", status, context, duration_ms, err),
            None => format!("{} ({}) in {}ms", status, context, duration_ms),
        }
    }

    pub fn request_command(&self) -> String {
        format!("{:?}", self.request.command)
    }

    pub fn response_command(&self) -> Option<String> {
        self.response.as_ref().map(|r| format!("{:?}", r.command))
    }
}