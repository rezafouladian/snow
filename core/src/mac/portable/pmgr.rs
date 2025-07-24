use crate::tickable::{Tickable, Ticks};
use anyhow::{anyhow, Result};
use crate::types::Byte;

const DEFAULT_LOW_LEVEL: u16 = 590-512;
const DEFAULT_CUTOFF_LEVEL: u16 = 574-512;
const DEFAULT_HICHG_LEVEL: u16 = 712-512;

enum State {
    Idle,
    GetCommand,
    GetLength,
    GetData,
}

pub struct Pmgr {
    // Low battery level
    low_level: u8,
    // Cutoff level
    cutoff_level: u8,
    // Hicharge level
    hichg_level: u8,
    // Contrast level
    contrast: u8,

    // PRAM storage
    pram: [Byte; 128],

    // Power plane
    power_plane: u8,
    
    state: State,
    
    pub(crate) pmreq: bool,
    pub(crate) pmack: bool,
}

impl Pmgr {
    pub(crate) fn new() -> Pmgr {
        Self {
            low_level: DEFAULT_LOW_LEVEL as u8,
            cutoff_level: DEFAULT_CUTOFF_LEVEL as u8,
            hichg_level: DEFAULT_HICHG_LEVEL as u8,

            contrast: 0x0F,

            pram: [0; 128],

            power_plane: 0x9F,
            
            state: State::Idle,
            
            pmreq: true,
            pmack: true,
        }
    }

    fn cmd(&mut self, cmd: Byte, len: Byte, data: Vec<Byte>) -> (Result<()>, Option<Vec<Byte>>) {
        match cmd {
            // Power control
            0x10 => (self.power_control_set(data[0]), None),
            // Power status
            0x18 => (Ok(()), Some(vec![self.power_control_get().unwrap()])),
            // ADB command
            0x20 => {
                (
                self.adb(data[0], data[1], data[2], data[3..].to_owned()),
                None
                )
            }
            // ADB status
            0x28 => todo!(),
            // Clock set
            0x30 => todo!(),
            // Write PRAM
            0x31 => todo!(),
            0x32 => todo!(),
            // Clock read
            0x38 => todo!(),
            // Read PRAM
            0x39 => todo!(),
            0x3A => self.xpram_read(data[0], data[1]),
            // Set contrast
            0x40 => (self.contrast_set(data[0]), None),
            // Read contrast
            0x48 => (Ok(()), Some(vec![self.contrast_get().unwrap()])),
            // Set modem
            0x50 => todo!(),
            // Read modem
            0x58 => todo!(),
            // Read battery TODO
            0x68 => (Ok(()), Some(vec![0x00])),
            // Sleep request
            0x70 => todo!(),
            // Read interrupts
            0x78 => todo!(),
            // Set wake up time
            0x80 => todo!(),
            // Clear wake up time
            0x82 => todo!(),
            // Read wake up time
            0x88 => todo!(),
            // Set sound
            0x90 => todo!(),
            // Read sound
            0x98 => todo!(),
            // Write internal memory
            0xE0 => todo!(),
            // Read internal memory
            0xE8 => todo!(),
            // Read firmware version
            0xEA => todo!(),
            // Run self test
            0xEC => (Ok(()), Some(vec![0x00])),
            // Soft reset
            0xEF => todo!(),
            _ => (Ok(()), None),
        }
    }

    fn power_control_set(&mut self, val: Byte) -> Result<()> {
        match val & 0x80 {
            // Turn devices on
            0x00 => {
                self.power_plane |= val & 0x7F;
                Ok(())
            }
            // Turn devices off
            0x80 => {
                self.power_plane ^= val & 0x7F;
                Ok(())
            }
            _ => unreachable!(),
        }
    }

    fn power_control_get(&mut self) -> Result<Byte> {
        Ok(self.power_plane & 0x7F)
    }

    fn adb(&mut self, cmd: Byte, flags: Byte, len: Byte, data: Vec<Byte>) -> Result<()> {
        Ok(())
    }

    fn adb_status(&mut self) -> Result<()> {
        Ok(())
    }

    // Read XPRAM
    fn xpram_read(&mut self, loc: Byte, len: Byte) -> (Result<()>, Option<Vec<Byte>>) {
        match loc + len {
            0x00..=0x7F => {
                (
                Ok(()),
                Some(self.pram[loc as usize..(loc+len) as usize].to_owned())
                )
            },
            _ => (Err(anyhow!("Invalid XPRAM location")), None)
        }
    }

    // Set contrast (not implemented)
    fn contrast_set(&mut self, val: Byte) -> Result<()> {
        match val {
            0x00..=0x1F => {
                self.contrast = val;
                Ok(())
            }
            _ => Err(anyhow!("Invalid contrast value"))
        }
    }

    // Read contrast (not implemented)
    fn contrast_get(&mut self) -> Result<Byte> {
        Ok(self.contrast)
    }
}

impl Tickable for Pmgr {
    fn tick(&mut self, ticks: Ticks) -> Result<Ticks> {
        match self.state {
            State::Idle => {
                if !self.pmreq {
                    
                }
            },
            State::GetCommand | State::GetLength | State::GetData => todo!()
        }
        
        Ok(ticks)
    }
}