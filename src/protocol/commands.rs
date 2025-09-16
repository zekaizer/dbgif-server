#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdbCommand {
    CNXN = 0x4E584E43,  // "CNXN"
    OKAY = 0x59414B4F,  // "OKAY"
    OPEN = 0x4E45504F,  // "OPEN"
    WRTE = 0x45545257,  // "WRTE"
    CLSE = 0x45534C43,  // "CLSE"
    PING = 0x474E4950,  // "PING"
    PONG = 0x474E4F50,  // "PONG"
}