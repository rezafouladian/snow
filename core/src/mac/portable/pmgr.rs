use crate::debuggable::Debuggable;
use crate::mac::adb::{AdbDevice, AdbDeviceInstance, AdbDeviceResponse};
use crate::tickable::{Tickable, Ticks};
use crate::types::Byte;
use anyhow::{anyhow, Result};
use chrono::{Local, NaiveDate};
use proc_bitfield::bitfield;

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
        swim_power: bool @ 0,
        scc_power: bool @ 1,
        hd_power: bool @ 2,
        modem_power: bool @ 3,
        serial_power: bool @ 4,
        sound_power: bool @ 5,
        negative_power: bool @ 6,
        sys_power: bool @ 7,
    }
}

bitfield! {
    struct ADBStatus(u8): {
        init: bool @ 0,
        new: bool @ 1,
        autopoll: bool @ 2,
        srq: bool @ 3,
        noreply: bool @ 4,
        error: bool @ 7,
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
    /// Low battery level
    low_level: u8,
    /// Cutoff level
    cutoff_level: u8,
    /// Hicharge level
    hichg_level: u8,
    /// Contrast level
    contrast: u8,

    /// PRAM storage
    pram: [Byte; 20],
    xpram: [Byte; 128],

    /// Power plane
    power_plane: PowerPlane,

    adb_status: ADBStatus,
    unknown_flags: UnknownFlags,
    interrupt_flags: InterruptFlags,
    power_flags: PowerFlags,

    time: u32,
    wake_time: [Byte; 4],

    /// The last ADB command
    last_adb: Byte,
    /// If ADB is initialized
    adb_ready: bool,

    adb_devices: Vec<AdbDeviceInstance>,
    adb_response: AdbDeviceResponse,
    adb_srq: bool,

    battery_level: u8,

    modem_ab: bool,

    state: State,

    timer1: usize,

    /// Whether the command is a read or write
    read: bool,
    cmd: Byte,
    length: Byte,
    data_pointer: usize,
    data: Vec<Byte>,
    wait_count: usize,

    current_adb_device: usize,

    pub(crate) pmreq: bool,
    pub(crate) pmack: bool,
    pub(crate) a_in: Byte,
    pub(crate) a_out: Byte,
    pub(crate) interrupt: bool,
    pub(crate) onesec: bool,
    onesec_latch: bool,

    adb_data_length: Byte,
    adb_data: Vec<Byte>,
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
            interrupt_flags: InterruptFlags(0b1000),
            power_flags: PowerFlags(0b1),

            wake_time: [0x00; 4],

            last_adb: 0x00,
            adb_ready: false,
            adb_srq: false,

            adb_devices: vec![],
            adb_response: AdbDeviceResponse::new(),

            battery_level: (720 - 512) as u8,

            modem_ab: true,

            state: State::Idle,

            timer1: 0,

            read: false,
            cmd: 0x00,
            length: 0x00,
            data_pointer: 0x00,
            data: vec![0; 20],
            wait_count: 0,

            pmreq: true,
            pmack: true,
            a_in: 0x00,
            a_out: 0x00,
            onesec: false,
            onesec_latch: false,

            interrupt: false,

            current_adb_device: 0x00,

            adb_data_length: 0x00,
            adb_data: vec![0; 2],

            time: Local::now()
                .naive_local()
                .signed_duration_since(
                    NaiveDate::from_ymd_opt(1904, 1, 1)
                        .unwrap()
                        .and_hms_opt(0, 0, 0)
                        .unwrap(),
                )
                .num_seconds() as u32,
        }
    }

    fn cmd(&mut self, cmd: Byte, len: Byte, data: Vec<Byte>) -> (Result<()>, Option<Vec<Byte>>) {
        match cmd {
            // Power control
            0x10 => self.power_control_set(data[0]),
            // Power status
            0x18 => self.power_control_get(),
            0x1B => self.power_control_get(),
            // ADB command
            0x20 => self.adb_cmd(data[0], data[1], data[2], data[3..].to_owned()),
            // ADB off TODO
            0x21 => self.adb_off(),
            // ADB status
            0x28 => self.adb_status(),
            // Clock set TODO
            0x30 => self.clock_set(data.to_vec()),
            // Write PRAM
            0x31 => self.pram_write(data[0..].to_owned()),
            // Write XPRAM
            0x32 => self.xpram_write(data[0], data[1], data[2..].to_owned()),
            // Clock read TODO
            0x38 => self.clock_read(),
            // Read PRAM
            0x39 => self.pram_read(),
            // Read XPRAM
            0x3A => self.xpram_read(data[0], data[1]),
            // Set contrast
            0x40 => self.contrast_set(data[0]),
            // Read contrast
            0x48 => self.contrast_get(),
            // Set modem
            0x50 => self.modem_set(data[0]),
            // Read modem
            0x58 => self.modem_get(),
            // Read battery
            0x68 => self.battery_read(),
            // Read battery with update
            0x69 => self.battery_read_now(),
            // Sleep request
            0x70 => self.sleep_request(data[0..=3].to_owned()),
            // Read interrupts
            0x78 => self.read_interrupts(),
            // Set wake up time
            0x80 => self.wake_set(data[0..=3].to_owned()),
            // Clear wake up time
            0x82 => self.wake_clear(),
            // Read wake up time
            0x88 => self.wake_read(),
            // Set sound
            0x90 => self.sound_set(),
            // Read sound
            0x98 => self.sound_read(),
            // Write internal memory
            0xE0 => self.internal_write(data[0], data[1], data[2..].to_owned()),
            // Read internal memory
            0xE8 => self.internal_read(data[0], data[1], data[2]),
            // Read firmware version
            0xEA => self.version_read(),
            // Run self test
            0xEC => self.self_test(),
            // Soft reset
            0xEF => self.soft_reset(),
            _ => {
                println!("Unknown command: {:X}", cmd);
                (Ok(()), None)
            }
        }
    }

    /// Turn devices on or off
    fn power_control_set(&mut self, val: Byte) -> (Result<()>, Option<Vec<Byte>>) {
        match val & 0x80 {
            // Turn devices on
            0x00 => {
                self.power_plane.0 |= val & 0x7F;
                (Ok(()), None)
            }
            // Turn devices off
            0x80 => {
                self.power_plane.0 ^= val & 0x7F;
                (Ok(()), None)
            }
            _ => unreachable!(),
        }
    }

    /// Get the power state of devices
    fn power_control_get(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 0x01;
        (Ok(()), Some(vec![self.power_plane.0 & 0x7F]))
    }

    /// Send an ADB command
    fn adb_cmd(
        &mut self,
        cmd: Byte,
        flags: Byte,
        len: Byte,
        data: Vec<Byte>,
    ) -> (Result<()>, Option<Vec<Byte>>) {
        self.last_adb = cmd;
        self.adb_status.0 = flags;
        self.adb_status.set_new(true);
        self.adb_data_length = len;
        self.adb_data = data[0..=len as usize].to_owned();

        self.interrupt_flags.set_adbint(false);
        self.adb_response.clear();

        (Ok(()), None)
    }

    /// Turn ADB off
    fn adb_off(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.interrupt_flags.set_adbint(false);
        (Ok(()), None)
    }

    /// Get ADB status
    fn adb_status(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.interrupt_flags.set_adbint(false);
        self.adb_status.set_noreply(false);

        if self.adb_status.srq() {
            if let Some(device) = self.adb_devices.iter_mut().find(|d| d.get_srq()) {
                self.adb_status.set_srq(false);
                self.adb_response = device.talk(0);
                self.last_adb = device.get_address() << 4 | 0x08;
            } else {
                self.adb_status.set_noreply(true);
                self.adb_status.set_srq(false);
            }
        }
        self.adb_data_length = self.adb_response.len() as Byte;

        self.length = 3 + self.adb_data_length;

        let mut result = vec![self.last_adb, self.adb_status.0, self.adb_data_length];
        result.extend(self.adb_response.to_owned());
        println!("ADB response: {:?}", result);
        (Ok(()), Some(result))
    }

    /// Set the current time
    fn clock_set(&mut self, time: Vec<Byte>) -> (Result<()>, Option<Vec<Byte>>) {
        if time.len() == 4 {
            let mut bytes = [0u8; 4];
            bytes.copy_from_slice(&time[0..4]);
            self.time = u32::from_be_bytes(bytes);
        }
        (Ok(()), None)
    }

    /// Write the initial 20 bytes of PRAM
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

    /// Get the current time
    fn clock_read(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 4;
        (Ok(()), Some(self.time.to_be_bytes().to_vec()))
    }

    /// Read the first 20 bytes of PRAM
    fn pram_read(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 20;
        (Ok(()), Some(self.pram[0..20].to_owned()))
    }

    /// Read XPRAM
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

    /// Set contrast
    fn contrast_set(&mut self, val: Byte) -> (Result<()>, Option<Vec<Byte>>) {
        match val {
            0x00..=0x1F => {
                self.contrast = val;
                (Ok(()), None)
            }
            // Bad contrast value
            _ => { (Ok(()), None) },
        }
    }

    /// Read contrast
    fn contrast_get(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 0x01;
        (Ok(()), Some(vec![self.contrast]))
    }

    fn modem_set(&mut self, val: Byte) -> (Result<()>, Option<Vec<Byte>>) {
        // TODO
        if val & 0x01 == 0x01 {
            self.power_plane.set_modem_power(true);
            self.power_plane.set_negative_power(true);
        } else {
            self.power_plane.set_modem_power(false);
            self.power_plane.set_negative_power(false);
        }
        if val & 0x02 == 0x02 {
            self.modem_ab = true;
        } else {
            self.modem_ab = false;
        }
        if val & 0x04 == 0x04 {
            self.unknown_flags.set_ring_wake_on(true);
        } else {
            self.unknown_flags.set_ring_wake_on(false);
        }
        (Ok(()), None)
    }

    fn modem_get(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 0x01;

        let mut modem_flags: Byte = 0x00;
        if self.power_plane.modem_power() & self.power_plane.negative_power() {
            modem_flags |= 0b000001;
        }
        if self.modem_ab {
            modem_flags |= 0b000010;
        }
        if self.unknown_flags.ring_wake_on() {
            modem_flags |= 0b000100;
        }
        // TODO modem installed
        // TODO ring detect
        // TODO modem on/off hook
        (Ok(()), Some(vec![modem_flags]))
    }

    /// Read power state, battery level as of last read, unused temp
    fn battery_read(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        // TODO
        if self.unknown_flags.unk1() {
            self.interrupt_flags.set_unimplemented(false);
            self.interrupt_flags.set_batint(false);
            self.power_flags.set_charger_changed(false);
        }
        self.length = 0x03;
        (
            Ok(()),
            Some(vec![self.power_flags.0, self.battery_level, 0xFF]),
        )
    }

    /// Read power info and get the latest battery level
    fn battery_read_now(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.battery_read()
    }

    fn sleep_request(&mut self, string: Vec<Byte>) -> (Result<()>, Option<Vec<Byte>>) {
        if string == b"MATT".to_vec() {
            // Sleep now
        } else {
            self.cmd = 0xAA;
        }
        (Ok(()), None)
    }

    /// Read interrupts from the power manager
    fn read_interrupts(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        let interrupt_flags = self.interrupt_flags.0;
        self.interrupt_flags.set_resetint(false);
        self.unknown_flags.set_unk1(false);
        self.length = 0x01;
        (Ok(()), Some(vec![interrupt_flags]))
    }

    /// Set the wake-up time
    fn wake_set(&mut self, time: Vec<Byte>) -> (Result<()>, Option<Vec<Byte>>) {
        self.unknown_flags.set_wake_time_on(true);
        self.wake_time = time.try_into().unwrap_or([0; 4]);
        (Ok(()), None)
    }

    /// Disable the wake-up time
    fn wake_clear(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.unknown_flags.set_wake_time_on(false);
        (Ok(()), None)
    }

    /// Get the current wake-up time
    fn wake_read(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 0x05;
        let mut wake_time = self.wake_time.to_vec();
        if self.unknown_flags.wake_time_on() {
            wake_time.push(0x01);
        } else {
            wake_time.push(0x00);
        }

        (Ok(()), Some(wake_time))
    }

    /// Set sound control bits
    fn sound_set(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        // TODO
        (Ok(()), None)
    }

    /// Read sound control bits
    fn sound_read(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        // TODO
        (Ok(()), Some(vec![0x00]))
    }

    fn internal_write(
        &mut self,
        _loch: Byte,
        _locl: Byte,
        _data: Vec<Byte>,
    ) -> (Result<()>, Option<Vec<Byte>>) {
        // TODO
        (Ok(()), None)
    }

    fn internal_read(
        &mut self,
        _loch: Byte,
        _locl: Byte,
        _len: Byte,
    ) -> (Result<()>, Option<Vec<Byte>>) {
        // TODO
        (Ok(()), None)
    }

    fn version_read(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 0x02;
        (Ok(()), Some(vec![0x02, 0xB5]))
    }

    fn self_test(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 0x01;
        (Ok(()), Some(vec![0x00]))
    }

    fn soft_reset(&mut self) -> (Result<()>, Option<Vec<Byte>>) {
        self.length = 0x00;
        self.interrupt_flags.set_resetint(true);
        (Ok(()), None)
    }

    /// Process a pending ADB command
    fn adb_cmd_do(&mut self) {
        // Command input only takes bits 0 and 2
        self.adb_status.0 &= 0b101;
        // Check for a reset command
        if self.last_adb & 0x0F == 0 {
            for dev in &mut self.adb_devices {
                dev.reset();
            }
            self.adb_status.set_noreply(true);
            self.interrupt_flags.set_adbint(true);
            return;
        }
        if self.adb_status.init() {
            self.interrupt_flags.set_adbint(true);
        }
        let Some(device) = self
            .adb_devices
            .iter_mut()
            .find(|d| d.get_address() == (self.last_adb >> 4))
        else {
            self.interrupt_flags.set_adbint(true);
            self.adb_status.set_noreply(true);
            return;
        };
        match self.last_adb & 0x0D {
            0x01 => {
                device.flush();
            }
            0x08 | 0x09 => {
                device.listen(self.last_adb & 3, &self.adb_data[1..]);
            }
            0x0C | 0x0D => {
                self.adb_response = device.talk(self.last_adb & 3);
            }
            _ => {
                println!("Unknown ADB command: {:X}", self.last_adb);
            }
        }
    }

    pub(crate) fn adb_add_device<T>(&mut self, device: T)
    where
        T: AdbDevice + Send + 'static,
    {
        self.adb_devices.push(Box::new(device));
    }
    
    pub(crate) fn onesec(&mut self) {
        
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

        if self.onesec & ! self.onesec_latch {
            self.time += 1;
            self.onesec_latch = true;
        } else if !self.onesec & self.onesec_latch {
            self.onesec_latch = false;
        }

        if self.interrupt_flags.0 != 0 {
            self.interrupt = true;
        } else {
            self.interrupt = false;
        }

        if self.adb_status.new() {
            self.adb_cmd_do();
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
                self.data = vec![0; 20];
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
