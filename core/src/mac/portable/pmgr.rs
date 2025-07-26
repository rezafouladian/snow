use std::ops::{BitOrAssign, BitXorAssign};
use crate::debuggable::Debuggable;
use crate::tickable::{Tickable, Ticks};
use crate::types::Byte;
use anyhow::{anyhow, Result};
use proc_bitfield::bitfield;
use crate::mac::adb::{AdbDevice, AdbDeviceInstance, AdbDeviceResponse};

const DEFAULT_LOW_LEVEL: u16 = 590 - 512;
const DEFAULT_CUTOFF_LEVEL: u16 = 574 - 512;
const DEFAULT_HICHG_LEVEL: u16 = 712 - 512;

bitfield! {
    pub struct UnknownFlags(u8): {
        pub unk1: bool @ 1,
        pub unk2: bool @ 2,
        pub wake_time_on: bool @ 4,
        pub unk5: bool @ 5,
        pub unk6: bool @ 6,
        pub ring_wake_on: bool @ 7,
    }
}

bitfield! {
    pub struct InterruptFlags(u8): {
        // ADB data waiting
        pub adbint: bool @ 0,
        // Low battery
        pub batint: bool @ 1,
        pub unimplemented: bool @ 2,
        pub resetint: bool @ 3,
    }
}

bitfield! {
    pub struct PowerFlags(u8): {
        pub charger_connected: bool @ 0,
        pub hichg: bool @ 1,
        pub hichg_overflow: bool @ 2,
        pub battery_dead: bool @ 3,
        pub battery_low: bool @ 4,
        pub charger_changed: bool @ 5,
        pub unk6: bool @ 6,
    }
}

bitfield! {
    pub struct PowerPlane(u8): {
        pub swim_power: bool @ 0,
        pub scc_power: bool @ 1,
        pub hd_power: bool @ 2,
        pub modem_power: bool @ 3,
        pub serial_power: bool @ 4,
        pub sound_power: bool @ 5,
        pub negative_power: bool @ 6,
        pub sys_power: bool @ 7,
    }
}

bitfield! {
    pub struct ADBStatus(u8): {
        pub init: bool @ 0,
        pub srq: bool @ 3,
    }
}

#[derive(Debug)]
enum State {
    Idle,
    GetCommand,
    WaitLength,
    GetLength,
    WaitData,
    GetData,
    WaitCommand,
    DoCommand,
    ReturnCmd,
    ReturnCmdWait,
    ReturnLength,
    ReturnDataWait,
    ReturnData,
    Cleanup,
    CleanupWait,
    Cleanup2,
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
    pram: [Byte; 20],
    xpram: [Byte; 128],

    // Power plane
    power_plane: PowerPlane,

    adb_status: ADBStatus,
    unknown_flags: UnknownFlags,
    interrupt_flags: InterruptFlags,
    power_flags: PowerFlags,

    last_adb: Byte,
    adb_ready: bool,

    adb_devices: Vec<AdbDeviceInstance>,
    adb_response: AdbDeviceResponse,
    adb_srq: bool,

    battery_level: u8,

    state: State,

    timer1: usize,

    // Whether the command is a read or write
    read: bool,
    cmd: Byte,
    length: Byte,
    data_pointer: usize,
    data: Vec<Byte>,
    wait_count: usize,

    pub(crate) pmreq: bool,
    pub(crate) pmack: bool,
    pub(crate) a_in: Byte,
    pub(crate) a_out: Byte,
    pub(crate) interrupt: bool,

    debug_flag: bool,
}

impl BitOrAssign<u8> for PowerPlane {
    fn bitor_assign(&mut self, rhs: u8) {
        *self = PowerPlane(self.0 | rhs);
    }
}

impl BitXorAssign<u8> for PowerPlane {
    fn bitxor_assign(&mut self, rhs: u8) {
        *self = PowerPlane(self.0 ^ rhs);
    }
}

impl Pmgr {
    pub(crate) fn new() -> Pmgr {
        Self {
            low_level: DEFAULT_LOW_LEVEL as u8,
            cutoff_level: DEFAULT_CUTOFF_LEVEL as u8,
            hichg_level: DEFAULT_HICHG_LEVEL as u8,

            contrast: 0x0F,

            pram: [0x00; 20],
            xpram: [0x00; 128],


            power_plane: PowerPlane(0x9F),

            adb_status: ADBStatus(0),
            unknown_flags: UnknownFlags(0),
            interrupt_flags: InterruptFlags(0),
            power_flags: PowerFlags(0b1),

            last_adb: 0x00,
            adb_ready: false,
            adb_srq: false,

            adb_devices: vec![],
            adb_response: AdbDeviceResponse::new(),

            battery_level: (720 - 512) as u8,

            state: State::Idle,

            timer1: 0,

            read: false,
            cmd: 0x00,
            length: 0x00,
            data_pointer: 0x00,
            data: vec![0; 32],
            wait_count: 0,

            pmreq: true,
            pmack: true,
            a_in: 0x00,
            a_out: 0x00,

            debug_flag: false,

            interrupt: false,
        }

    }

    fn cmd(&mut self, cmd: Byte, len: Byte, data: Vec<Byte>) -> (Result<()>, Option<Vec<Byte>>) {
        match cmd {
            // Power control
            0x10 => self.power_control_set(data[0]),
            // Power status
            0x18 => (Ok(()), Some(vec![self.power_control_get().unwrap()])),
            // ADB command
            0x20 => self.adb(data[0], data[1], data[2], data[3..].to_owned()),
            // ADB off TODO
            0x21 => self.adb_off(),
            // ADB status
            0x28 => self.adb_status(),
            // Clock set TODO
            0x30 => (Ok(()), None),
            // Write PRAM
            0x31 => self.pram_write(data[0..].to_owned()),
            // Write XPRAM
            0x32 => self.xpram_write(data[0], data[1], data[2..].to_owned()),
            // Clock read TODO
            0x38 => (Ok(()), Some(vec![0x00; 4])),
            // Read PRAM
            0x39 => self.pram_read(),
            // Read XPRAM
            0x3A => self.xpram_read(data[0], data[1]),
            // Set contrast
            0x40 => (self.contrast_set(data[0]), None),
            // Read contrast
            0x48 => (Ok(()), Some(vec![self.contrast_get().unwrap()])),
            // Set modem
            0x50 => todo!(),
            // Read modem TODO
            0x58 => (Ok(()), Some(vec![0x00])),
            // Read battery with update (unused but valid command)
            0x60..=0x67 | 0x6A..=0x6F => todo!(),
            // Read battery
            0x68 => self.battery_read(),
            // Read battery with update
            0x69 => self.battery_read(),
            // Sleep request
            0x70 => todo!(),
            // Read interrupts TODO
            0x78 => self.read_interrupts(),
            // Set wake up time
            0x80 => todo!(),
            // Clear wake up time
            0x82 => todo!(),
            // Read wake up time
            0x88 => todo!(),
            // Set sound TODO
            0x90 => (Ok(()), None),
            // Read sound TODO
            0x98 => (Ok(()), Some(vec![0x00])),
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
            _ => {
                println!("Unknown command: {:X}", cmd);
                (Ok(()), None)
            }
        }
    }

    fn power_control_set(&mut self, val: Byte) -> (Result<()>, Option<Vec<Byte>>) {
        match val & 0x80 {
            // Turn devices on
            0x00 => {
                self.power_plane |= val & 0x7F;
                (Ok(()), None)
            }
            // Turn devices off
            0x80 => {
                self.power_plane ^= val & 0x7F;
                (Ok(()), None)
            }
            _ => unreachable!(),
        }
    }

    fn power_control_get(&mut self) -> Result<Byte> {
        self.length = 0x01;
        Ok(self.power_plane.0 & 0x7F)
    }

    fn adb(&mut self, cmd: Byte, flags: Byte, len: Byte, data: Vec<Byte>) -> (Result<()>, Option<Vec<Byte>>) {
        self.adb_response.clear();
        self.interrupt_flags.set_adbint(false);
        self.last_adb = cmd;
        if flags == 1 {
            self.adb_ready = true;
            self.interrupt_flags.set_adbint(true);
        }
        let Some(device) = self.adb_devices.iter_mut().find(|d| d.get_address() == (cmd >> 4)) else {
            return (Ok(()), None);
        };
        match cmd & 0x0D {
            0x00 => {
                for dev in &mut self.adb_devices {
                    dev.reset();
                }
            }
            0x01 => {
                device.flush();
            }
            0x08 => {
                device.listen(cmd & 3, &data[1..]);
            }
            0x0C => {
                self.adb_response = device.talk(cmd & 3);
            }
            _ => {}
        }
        (Ok(()), None)
    }

    fn adb_off(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.interrupt_flags.set_adbint(false);
        (Ok(()), None)
    }

    fn adb_status(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        // TEMP

        // Find device with srq
        if self.adb_status.srq() {
            let Some(device) = self.adb_devices.iter_mut().find(|d| d.get_srq()) else {
                return (Ok(()), None);
            };
            self.adb_response = device.talk(0);
        }
        let response = self.adb_response.pop_at(0).unwrap_or(0);
        let length = 1;

        self.interrupt_flags.set_adbint(false);
        self.length = 3 + length;
        (Ok(()), Some(vec![self.last_adb, self.adb_status.0, length, response]))
    }

    fn pram_write(&mut self, data: Vec<Byte>) -> (Result<()>, Option<Vec<Byte>>) {
        for i in 0..20 {
            self.pram[i] = data[i];
        }
        (Ok(()), None)
    }

    fn xpram_write(
        &mut self,
        loc: Byte,
        len: Byte,
        data: Vec<Byte>,
    ) -> (Result<()>, Option<Vec<Byte>>) {
        match loc + len - 1 {
            0x00..=0x7F => {
                for i in 0..len as usize {
                    self.xpram[loc as usize + i] = data[i];
                }
                (Ok(()), None)
            }
            _ => {
                println!("Invalid XPRAM location: {:X}", loc);
                (Err(anyhow!("Invalid XPRAM location")), None)
            }
        }
    }

    // Read the first 20 bytes of PRAM
    fn pram_read(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 20;
        (Ok(()), Some(self.pram[0..20].to_owned()))
    }

    // Read XPRAM
    fn xpram_read(&mut self, loc: Byte, len: Byte) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = len;
        match loc + len - 1 {
            0x00..=0x7F => {
                self.length = len;
                (
                    Ok(()),
                    Some(self.xpram[loc as usize..(loc + len) as usize].to_owned()),
                )
            }
            _ => {
                println!("Invalid XPRAM location: {:X}", loc);
                (Err(anyhow!("Invalid XPRAM location")), None)
            }
        }
    }

    // Set contrast (not implemented)
    fn contrast_set(&mut self, val: Byte) -> Result<()> {
        match val {
            0x00..=0x1F => {
                self.contrast = val;
                Ok(())
            }
            _ => Err(anyhow!("Invalid contrast value")),
        }
    }

    // Read contrast (not implemented)
    fn contrast_get(&mut self) -> Result<Byte> {
        self.length = 0x01;
        Ok(self.contrast)
    }

    fn battery_read(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        // TODO
        let power_flags = self.power_flags.0;
        if self.unknown_flags.unk1() {
            self.interrupt_flags.set_unimplemented(false);
            self.interrupt_flags.set_batint(false);
            self.power_flags.set_charger_changed(false);
        }
        self.length = 0x03;
        (
            Ok(()),
            Some(vec![power_flags, self.battery_level, 0xA0]),
        )
    }

    fn read_interrupts(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        let interrupt_flags = self.interrupt_flags.0;
        self.interrupt_flags.set_resetint(false);
        self.unknown_flags.set_unk1(false);
        self.length = 0x01;
        (Ok(()), Some(vec![interrupt_flags]))
    }

    fn wake_set(&mut self, time: Byte) -> (Result<()>, Option<Vec<Byte>>) {
        // TODO
        self.unknown_flags.set_wake_time_on(true);
        (Ok(()), None)
    }

    fn wake_clear(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.unknown_flags.set_wake_time_on(false);
        (Ok(()), None)
    }

    fn wake_read(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        // TODO
        if !self.unknown_flags.wake_time_on() {

        }
        (Ok(()), Some(vec![0x00]))
    }

    pub(crate) fn adb_add_device<T>(&mut self, device: T)
    where
        T: AdbDevice + Send + 'static,
    {
        self.adb_devices.push(Box::new(device));
    }

    pub(crate) fn reset(&mut self) {
        self.state = State::Idle;
        self.read = false;
        self.cmd = 0x00;
        self.length = 0x00;
        self.pmack = true;
        self.pmreq = true;
        self.adb_ready = false;
        self.adb_status.set_srq(false);
        self.interrupt_flags.set_adbint(false);
    }
}

impl Tickable for Pmgr {
    fn tick(&mut self, ticks: Ticks) -> Result<Ticks> {
        match self.timer1 {
            1 => {
                self.timer1 -= 1;
            }
            0 => {}
            _ => {
                self.timer1 -= 1;
            }
        }

        if self.interrupt_flags.0 != 0 {
            self.interrupt = true;
        } else {
            self.interrupt = false;
        }

        if self.adb_devices.iter().any(|d| d.get_srq()) & !self.interrupt_flags.adbint() {
            self.interrupt_flags.set_adbint(true);
            self.adb_status.set_srq(true);
        }

        match self.state {
            State::Idle => {
                if !self.pmreq {
                    self.state = State::GetCommand;
                }
            }
            State::GetCommand => {
                if self.pmack {
                    let cmd = self.a_out;
                    self.read = cmd & 0x08 == 0x08;
                    self.cmd = cmd;
                    self.pmack = false;
                    if cmd == 0x00 {
                        self.pmack = true;
                        self.state = State::Idle;
                    }
                }
                if self.pmreq {
                    self.pmack = true;
                    self.state = State::WaitLength;
                }
            }
            State::WaitLength => {
                if !self.pmreq {
                    self.state = State::GetLength;
                }
            }
            State::GetLength => {
                if self.pmack {
                    self.length = self.a_out;
                    self.pmack = false;
                }
                if self.pmreq {
                    self.pmack = true;
                    if self.length == 0 {
                        self.wait_count = 10;
                        self.state = State::WaitCommand;
                    } else {
                        self.state = State::WaitData;
                    }
                }
            }
            State::WaitData => {
                if !self.pmreq {
                    self.state = State::GetData;
                }
            }
            State::GetData => {
                if self.pmack {
                    self.data[self.data_pointer] = self.a_out;
                    self.pmack = false;
                }
                if self.pmreq {
                    self.pmack = true;
                    self.data_pointer += 1;
                    if self.data_pointer >= self.length as usize {
                        self.wait_count = 10;
                        self.data_pointer = 0;
                        self.state = State::WaitCommand;
                    } else {
                        self.state = State::WaitData;
                    }
                }
            }
            State::WaitCommand => {
                if self.wait_count <= 0 {
                    self.state = State::DoCommand;
                } else {
                    self.wait_count -= 1;
                }
            }
            State::DoCommand => {
                self.data = self
                    .cmd(self.cmd, self.length, self.data.to_owned())
                    .1
                    .unwrap_or(vec![0; 4]);
                if self.read {
                    self.state = State::ReturnCmd;
                } else {
                    self.state = State::Cleanup;
                }
            }
            State::ReturnCmd => {
                if self.pmreq & self.pmack {
                    self.pmack = false;
                    self.a_in = self.cmd;
                }
                if !self.pmreq {
                    self.pmack = true;
                    self.wait_count = 100;
                    self.state = State::ReturnCmdWait;
                }
            }
            State::ReturnCmdWait => {
                if self.wait_count <= 0 {
                    self.state = State::ReturnLength;
                } else {
                    self.wait_count -= 1;
                }
            }
            State::ReturnLength => {
                if self.pmreq & self.pmack {
                    self.pmack = false;
                    self.a_in = self.length;
                }
                if !self.pmreq {
                    self.pmack = true;
                    // if self.length > 4 { self.length = 4; }
                    if self.length == 0 {
                        self.state = State::Cleanup;
                    } else {
                        self.wait_count = 100;
                        self.state = State::ReturnDataWait;
                    }
                }
            }
            State::ReturnDataWait => {
                if self.wait_count <= 0 {
                    self.state = State::ReturnData;
                } else {
                    self.wait_count -= 1;
                }
            }
            State::ReturnData => {
                if self.pmreq & self.pmack {
                    self.pmack = false;
                    self.a_in = self.data[self.data_pointer];
                }
                if !self.pmreq {
                    self.pmack = true;
                    self.data_pointer += 1;
                    if self.data_pointer >= self.length as usize {
                        self.state = State::Cleanup;
                    } else {
                        self.wait_count = 100;
                        self.state = State::ReturnDataWait;
                    }
                }
            }
            State::Cleanup => {
                self.cmd = 0x00;
                self.length = 0x00;
                self.data_pointer = 0x00;
                self.data = vec![0; 32];
                self.wait_count = 100;
                self.state = State::CleanupWait;
            }
            State::CleanupWait => {
                if self.wait_count <= 0 {
                    self.state = State::Cleanup2;
                } else {
                    self.wait_count -= 1;
                }
            }
            // Pull the communication bus high
            State::Cleanup2 => {
                self.a_in = 0xFF;
                self.state = State::Idle;
            }
        }

        Ok(ticks)
    }
}

impl Debuggable for Pmgr {
    fn get_debug_properties(&self) -> crate::debuggable::DebuggableProperties {
        use crate::debuggable::*;
        use crate::{dbgprop_bool, dbgprop_group, dbgprop_string};

        vec![
            dbgprop_string!("State", format!("{:?}", self.state)),
            dbgprop_string!(
                "PMREQ*",
                if self.pmreq {
                    "deasserted".to_string()
                } else {
                    "asserted".to_string()
                }
            ),
            dbgprop_string!(
                "PMACK*",
                if self.pmack {
                    "deasserted".to_string()
                } else {
                    "asserted".to_string()
                }
            ),
            dbgprop_bool!("SWIM Power", !self.power_plane.swim_power()),
            dbgprop_bool!("SCC Power", !self.power_plane.scc_power()),
            dbgprop_bool!("HD Power", !self.power_plane.hd_power()),
            dbgprop_bool!("Modem Power", !self.power_plane.modem_power()),
            dbgprop_bool!("Serial Power", !self.power_plane.serial_power()),
            dbgprop_bool!("Sound Latch", !self.power_plane.sound_power()),
            dbgprop_bool!("-5V Power", !self.power_plane.negative_power()),
            dbgprop_group!(
                "PRAM Contents",
                (0..2)
                    .map(|row| {
                        dbgprop_string!(
                            format!("{:02X}", row * 16),
                            (0..16)
                                .map(|col| {
                                    if row * 16 + col < 20 {
                                        format!("{:02X}", self.pram[row * 16 + col])
                                    } else {
                                        "  ".to_string()
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    })
                    .collect()
            ),
            dbgprop_group!(
                "XPRAM Contents",
                (0..8)
                    .map(|row| {
                        dbgprop_string!(
                            format!("{:02X}", row * 16),
                            (0..16)
                                .map(|col| format!("{:02X}", self.xpram[row * 16 + col]))
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    })
                    .collect()
            ),
            dbgprop_bool!("Power Manager Interrupt", self.interrupt),
            dbgprop_bool!("ADB SRQ", self.adb_devices.iter().any(|d| d.get_srq())),
        ]
    }
}
