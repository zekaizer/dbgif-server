/// Simplified debug implementation with better feature flag support
use async_trait::async_trait;
use anyhow::Result;

use crate::protocol::Message;
use super::{Transport, TransportType};

#[cfg(feature = "transport-debug")]
mod debug_enabled {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tracing::{debug, trace, info};
    use crate::utils::hex_dump::{hex_dump_string, format_bytes_inline};

    /// Debug wrapper with full functionality
    pub struct DebugTransport {
        inner: Box<dyn Transport + Send>,
        debug_enabled: Arc<AtomicBool>,
        device_id: String,
    }

    impl DebugTransport {
        pub fn new(transport: Box<dyn Transport + Send>) -> Self {
            let device_id = transport.device_id().to_string();
            Self {
                inner: transport,
                debug_enabled: Arc::new(AtomicBool::new(false)),
                device_id,
            }
        }

        pub fn with_debug(transport: Box<dyn Transport + Send>, enabled: bool) -> Self {
            let device_id = transport.device_id().to_string();
            Self {
                inner: transport,
                debug_enabled: Arc::new(AtomicBool::new(enabled)),
                device_id,
            }
        }

        #[inline]
        pub fn enable_debug(&self) {
            self.debug_enabled.store(true, Ordering::Relaxed);
            info!("Debug logging enabled for transport: {}", self.device_id);
        }

        #[inline]
        pub fn disable_debug(&self) {
            self.debug_enabled.store(false, Ordering::Relaxed);
            info!("Debug logging disabled for transport: {}", self.device_id);
        }

        #[inline(always)]
        pub fn is_debug_enabled(&self) -> bool {
            self.debug_enabled.load(Ordering::Relaxed)
        }

        #[inline]
        fn log_raw_message(&self, direction: &str, message: &Message) {
            if !self.is_debug_enabled() {
                return;
            }

            let raw_data = message.serialize();
            
            debug!(
                "{} [{}] {} - {} bytes: cmd={:?}(0x{:08X}) arg0=0x{:08X} arg1=0x{:08X} data_len={}",
                direction,
                self.device_id,
                self.inner.transport_type(),
                raw_data.len(),
                message.command,
                message.command.to_u32(),
                message.arg0,
                message.arg1,
                message.data.len()
            );

            if raw_data.len() <= 64 {
                debug!("{} [{}] Raw: {}", direction, self.device_id, format_bytes_inline(&raw_data, Some(32)));
            } else {
                debug!("{} [{}] Raw (first 32 bytes): {}", direction, self.device_id, format_bytes_inline(&raw_data, Some(32)));
            }

            if tracing::enabled!(tracing::Level::TRACE) {
                let hex_dump = hex_dump_string(&format!("{} [{}]", direction, self.device_id), &raw_data, Some(256));
                trace!("\n{}", hex_dump);
            }
        }
    }

    #[async_trait]
    impl Transport for DebugTransport {
        #[inline]
        async fn send_message(&mut self, message: &Message) -> Result<()> {
            if self.is_debug_enabled() {
                self.log_raw_message("TX", message);
            }
            
            let result = self.inner.send_message(message).await;
            
            if self.is_debug_enabled() {
                match &result {
                    Ok(_) => debug!("TX [{}] Message sent successfully", self.device_id),
                    Err(e) => debug!("TX [{}] Send failed: {}", self.device_id, e),
                }
            }
            
            result
        }

        #[inline]
        async fn receive_message(&mut self) -> Result<Message> {
            let result = self.inner.receive_message().await;
            
            match &result {
                Ok(message) => {
                    if self.is_debug_enabled() {
                        self.log_raw_message("RX", message);
                        debug!("RX [{}] Message received successfully", self.device_id);
                    }
                }
                Err(e) => {
                    if self.is_debug_enabled() {
                        debug!("RX [{}] Receive failed: {}", self.device_id, e);
                    }
                }
            }
            
            result
        }

        #[inline]
        async fn connect(&mut self) -> Result<()> {
            if self.is_debug_enabled() {
                debug!("CONN [{}] Attempting to connect...", self.device_id);
            }
            
            let result = self.inner.connect().await;
            
            if self.is_debug_enabled() {
                match &result {
                    Ok(_) => info!("CONN [{}] Connected successfully", self.device_id),
                    Err(e) => debug!("CONN [{}] Connection failed: {}", self.device_id, e),
                }
            }
            
            result
        }

        #[inline]
        async fn disconnect(&mut self) -> Result<()> {
            if self.is_debug_enabled() {
                debug!("DISC [{}] Disconnecting...", self.device_id);
            }
            
            let result = self.inner.disconnect().await;
            
            if self.is_debug_enabled() {
                match &result {
                    Ok(_) => info!("DISC [{}] Disconnected successfully", self.device_id),
                    Err(e) => debug!("DISC [{}] Disconnect failed: {}", self.device_id, e),
                }
            }
            
            result
        }

        #[inline]
        async fn is_connected(&self) -> bool {
            self.inner.is_connected().await
        }

        #[inline]
        fn device_id(&self) -> &str {
            &self.device_id
        }

        #[inline]
        fn transport_type(&self) -> TransportType {
            self.inner.transport_type()
        }

        #[inline]
        async fn health_check(&self) -> Result<()> {
            if self.is_debug_enabled() {
                debug!("HEALTH [{}] Running health check...", self.device_id);
            }
            
            let result = self.inner.health_check().await;
            
            if self.is_debug_enabled() {
                match &result {
                    Ok(_) => debug!("HEALTH [{}] Health check passed", self.device_id),
                    Err(e) => debug!("HEALTH [{}] Health check failed: {}", self.device_id, e),
                }
            }
            
            result
        }
    }
}

#[cfg(not(feature = "transport-debug"))]
mod debug_disabled {
    use super::*;

    /// Zero-cost debug wrapper when feature is disabled
    pub struct DebugTransport {
        inner: Box<dyn Transport + Send>,
    }

    impl DebugTransport {
        #[inline(always)]
        pub fn new(transport: Box<dyn Transport + Send>) -> Self {
            Self { inner: transport }
        }

        #[inline(always)]
        pub fn with_debug(transport: Box<dyn Transport + Send>, _enabled: bool) -> Self {
            Self { inner: transport }
        }

        #[inline(always)]
        pub fn enable_debug(&self) {}

        #[inline(always)]
        pub fn disable_debug(&self) {}

        #[inline(always)]
        pub const fn is_debug_enabled(&self) -> bool {
            false
        }
    }

    #[async_trait]
    impl Transport for DebugTransport {
        #[inline(always)]
        async fn send_message(&mut self, message: &Message) -> Result<()> {
            self.inner.send_message(message).await
        }

        #[inline(always)]
        async fn receive_message(&mut self) -> Result<Message> {
            self.inner.receive_message().await
        }

        #[inline(always)]
        async fn connect(&mut self) -> Result<()> {
            self.inner.connect().await
        }

        #[inline(always)]
        async fn disconnect(&mut self) -> Result<()> {
            self.inner.disconnect().await
        }

        #[inline(always)]
        async fn is_connected(&self) -> bool {
            self.inner.is_connected().await
        }

        #[inline(always)]
        fn device_id(&self) -> &str {
            self.inner.device_id()
        }

        #[inline(always)]
        fn transport_type(&self) -> TransportType {
            self.inner.transport_type()
        }

        #[inline(always)]
        async fn health_check(&self) -> Result<()> {
            self.inner.health_check().await
        }
    }
}

// Re-export the appropriate implementation
#[cfg(feature = "transport-debug")]
pub use debug_enabled::DebugTransport;

#[cfg(not(feature = "transport-debug"))]
pub use debug_disabled::DebugTransport;

/// Helper function to check if debug mode is enabled via environment variable
#[inline]
pub fn is_debug_env_enabled() -> bool {
    #[cfg(feature = "transport-debug")]
    {
        std::env::var("DBGIF_DEBUG").is_ok() || 
        std::env::var("DBGIF_DEBUG_TRANSPORT").is_ok()
    }
    
    #[cfg(not(feature = "transport-debug"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_detection() {
        // Test without env var
        if std::env::var("DBGIF_DEBUG").is_err() && std::env::var("DBGIF_DEBUG_TRANSPORT").is_err() {
            #[cfg(not(feature = "transport-debug"))]
            assert!(!is_debug_env_enabled());
        }
    }
}