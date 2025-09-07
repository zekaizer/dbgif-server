// dbgif protocol constants
pub const VERSION: u32 = 0x01000000;
pub const MAXDATA: usize = 256 * 1024; // 256KB
pub const DEFAULT_PORT: u16 = 5037;

// Command codes
pub const CNXN: u32 = 0x4e584e43;
pub const OPEN: u32 = 0x4e45504f;
pub const OKAY: u32 = 0x59414b4f;
pub const WRTE: u32 = 0x45545257;
pub const CLSE: u32 = 0x45534c43;
pub const AUTH: u32 = 0x48545541;
pub const PING: u32 = 0x474e4950;
pub const PONG: u32 = 0x474e4f50;

// Magic values (bitwise NOT of command)
pub const CNXN_MAGIC: u32 = !CNXN;
pub const OPEN_MAGIC: u32 = !OPEN;
pub const OKAY_MAGIC: u32 = !OKAY;
pub const WRTE_MAGIC: u32 = !WRTE;
pub const CLSE_MAGIC: u32 = !CLSE;
pub const AUTH_MAGIC: u32 = !AUTH;
pub const PING_MAGIC: u32 = !PING;
pub const PONG_MAGIC: u32 = !PONG;