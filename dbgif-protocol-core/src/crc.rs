use crc32fast::Hasher;

pub struct Crc32State {
    hasher: Hasher,
}

impl Crc32State {
    pub fn new() -> Self {
        Self {
            hasher: Hasher::new(),
        }
    }

    pub fn reset(&mut self) {
        self.hasher = Hasher::new();
    }

    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    pub fn finalize(&self) -> u32 {
        self.hasher.clone().finalize()
    }
}

pub fn calculate_crc32(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}