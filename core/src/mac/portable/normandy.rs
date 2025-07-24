//! Normandy decoder implementation for the Macintosh Portable and
//! PowerBook 100.

use crate::bus::{Address, BusMember};

pub struct Normandy {
    
}

impl Normandy {
    pub(crate) fn new() -> Self {
        Self {
            
        }
    }
}

impl BusMember<Address> for Normandy {
    fn read(&mut self, addr: Address) -> Option<u8> {
        todo!()
    }

    fn write(&mut self, addr: Address, val: u8) -> Option<()> {
        todo!()
    }
}