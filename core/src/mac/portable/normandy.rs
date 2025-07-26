//! Normandy decoder implementation for the Macintosh Portable and
//! PowerBook 100.

use proc_bitfield::bitfield;
use crate::bus::{Address, BusMember};


bitfield! {
    #[derive(Clone)]
    pub struct SlimMapper(u8): {
        pub bit0: bool @ 0,
        pub bit1: bool @ 1,
        pub bit2: bool @ 2,
    }
}

pub struct Normandy {
    idle_speed: bool,
    slim_dtack: bool,
    slim_mapper: Vec<SlimMapper>
}

impl Normandy {
    pub(crate) fn new() -> Self {
        Self {
            idle_speed: false,
            slim_dtack: false,
            slim_mapper: vec![SlimMapper(0); 16]
        }
    }
}

impl BusMember<Address> for Normandy {
    fn read(&mut self, addr: Address) -> Option<u8> {
        match addr {
            0xF0_0000..=0xF0_FFFF => {
                Some(0x00)
            }
            0xFC_0000..=0xFC_FFFF => {
                match addr & 0x21F {
                    0x000..=0x01F => {
                        if addr & 0x1 != 0 {
                            Some(self.slim_mapper[((addr & 0x1F) >> 1) as usize].0)
                        } else {
                            Some(0x00)
                        }
                    }
                    0x200..=0x201 => {
                        self.slim_dtack = true;
                        Some(0x00)
                    }
                    0x202..=0x203 => {
                        Some(0x00)
                    }
                    _ => { None }
                }
            }
            0xFE_0000..=0xFE_FFFF => {
                match addr & 0x202 {
                    0x000 => {
                        self.idle_speed = false;
                        Some(0xFF)
                    }
                    0x002 => {
                        self.idle_speed = true;
                        Some(0xFF)
                    }
                    _ => { None }
                }
            }
            _ => { None }
        }
        
    }

    fn write(&mut self, addr: Address, val: u8) -> Option<()> {
        match addr {
            0xF0_0000..=0xF0_FFFF => {
                Some(())
            }
            0xFC_0000..=0xFC_FFFF => {
                match addr & 0x21F {
                    0x000..=0x01F => {
                        if addr & 0x1 != 0 {
                            self.slim_mapper[((addr & 0x1F) >> 1) as usize].0 = val & 0x7;
                            Some(())
                        } else {
                            Some(())
                        }
                    }
                    0x200..=0x201 => {
                        self.slim_dtack = true;
                        Some(())
                    }
                    0x202..=0x203 => {
                        Some(())
                    }
                    _ => { None }
                }
            }
            0xFE_0000..=0xFE_FFFF => {
                match addr & 0x202 {
                    0x000 => {
                        self.idle_speed = false;
                        Some(())
                    }
                    0x002 => {
                        self.idle_speed = true;
                        Some(())
                    }
                    _ => { None }
                }
            }
            _ => { None }
        }
    }
}