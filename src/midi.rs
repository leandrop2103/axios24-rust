use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiMsg {
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}

impl MidiMsg {
    pub fn kind(self) -> u8 {
        self.status & 0xF0
    }
    pub fn channel(self) -> u8 {
        self.status & 0x0F
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiBinding {
    pub channel: u8,
    pub is_note: bool,
    pub number: u8,
    pub target_flat_id: usize,
    pub is_mute: bool,
}

impl MidiBinding {
    pub fn label(&self) -> String {
        let kind = if self.is_note { "Note" } else { "CC" };
        let act = if self.is_mute { "Mute" } else { "Fader" };
        format!("Ch{} {} #{} -> ID {} ({})", self.channel + 1, kind, self.number, self.target_flat_id, act)
    }
}

pub struct MidiControllerEngine {
    pub connected_name: Option<String>,
    pub available_devices: Vec<String>,
    pub selected_idx: usize,
    pub bindings: Vec<MidiBinding>,
    pub learn_target: Option<(usize, bool)>,
    pub last_event_label: String,
    rx: Option<Receiver<MidiMsg>>,
    #[cfg(windows)]
    handle: usize,
    #[cfg(target_os = "linux")]
    linux_conn: Option<midir::MidiInputConnection<()>>,
}

impl Default for MidiControllerEngine {
    fn default() -> Self {
        let devices = list_midi_input_devices();
        // Default Mackie/Korg-style CC 1..8 -> Layer slots, CC 9 -> MAIN L/R
        Self {
            connected_name: None,
            available_devices: devices,
            selected_idx: 0,
            bindings: Vec::new(),
            learn_target: None,
            last_event_label: "Nenhum evento MIDI recebido".to_string(),
            rx: None,
            #[cfg(windows)]
            handle: 0,
            #[cfg(target_os = "linux")]
            linux_conn: None,
        }
    }
}

#[cfg(windows)]
mod win_midi {
    use super::MidiMsg;
    use std::ffi::c_void;
    use std::sync::mpsc::Sender;

    #[repr(C)]
    pub struct MidiInCapsA {
        pub w_mid: u16,
        pub w_pid: u16,
        pub v_driver_version: u32,
        pub sz_pname: [i8; 32],
        pub dw_support: u32,
    }

    pub const CALLBACK_FUNCTION: u32 = 0x0003_0000;
    pub const MIM_DATA: u32 = 0x3C3;

    #[link(name = "winmm")]
    extern "system" {
        pub fn midiInGetNumDevs() -> u32;
        pub fn midiInGetDevCapsA(uDeviceID: usize, pmic: *mut MidiInCapsA, cbmic: u32) -> u32;
        pub fn midiInOpen(
            phmi: *mut usize,
            uDeviceID: u32,
            dwCallback: usize,
            dwInstance: usize,
            fdwOpen: u32,
        ) -> u32;
        pub fn midiInStart(hmi: usize) -> u32;
        pub fn midiInStop(hmi: usize) -> u32;
        pub fn midiInClose(hmi: usize) -> u32;
    }

    pub unsafe extern "system" fn midi_in_proc(
        _hmi: usize,
        w_msg: u32,
        dw_instance: usize,
        dw_param1: usize,
        _dw_param2: usize,
    ) {
        if w_msg == MIM_DATA && dw_instance != 0 {
            let tx = &*(dw_instance as *const Sender<MidiMsg>);
            let status = (dw_param1 & 0xFF) as u8;
            let data1 = ((dw_param1 >> 8) & 0xFF) as u8;
            let data2 = ((dw_param1 >> 16) & 0xFF) as u8;
            let _ = tx.send(MidiMsg { status, data1, data2 });
        }
        let _ = std::mem::size_of::<*const c_void>();
    }
}

pub fn list_midi_input_devices() -> Vec<String> {
    #[cfg(windows)]
    unsafe {
        let count = win_midi::midiInGetNumDevs();
        let mut list = Vec::new();
        for i in 0..count {
            let mut caps = std::mem::zeroed::<win_midi::MidiInCapsA>();
            if win_midi::midiInGetDevCapsA(
                i as usize,
                &mut caps,
                std::mem::size_of::<win_midi::MidiInCapsA>() as u32,
            ) == 0
            {
                let bytes: Vec<u8> = caps
                    .sz_pname
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                list.push(String::from_utf8_lossy(&bytes).to_string());
            }
        }
        list
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(midi_in) = midir::MidiInput::new("axios24") {
            return midi_in
                .ports()
                .iter()
                .enumerate()
                .map(|(i, p)| midi_in.port_name(p).unwrap_or_else(|_| format!("MIDI {}", i + 1)))
                .collect();
        }
        Vec::new()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Vec::new()
    }
}

impl MidiControllerEngine {
    pub fn refresh_devices(&mut self) {
        self.available_devices = list_midi_input_devices();
        if self.selected_idx >= self.available_devices.len() {
            self.selected_idx = 0;
        }
    }

    pub fn connect(&mut self, idx: usize) -> Result<String, String> {
        self.disconnect();
        self.refresh_devices();
        let Some(name) = self.available_devices.get(idx).cloned() else {
            return Err("Nenhum controlador MIDI USB encontrado.".to_string());
        };
        #[cfg(windows)]
        unsafe {
            let (tx, rx) = mpsc::channel();
            let tx_ptr = Box::into_raw(Box::new(tx)) as usize;
            let mut hmi: usize = 0;
            let res = win_midi::midiInOpen(
                &mut hmi,
                idx as u32,
                win_midi::midi_in_proc as *const () as usize,
                tx_ptr,
                win_midi::CALLBACK_FUNCTION,
            );
            if res != 0 {
                let _ = Box::from_raw(tx_ptr as *mut Sender<MidiMsg>);
                return Err(format!("Erro ao abrir porta MIDI '{}' (código {})", name, res));
            }
            win_midi::midiInStart(hmi);
            self.handle = hmi;
            self.rx = Some(rx);
            self.connected_name = Some(name.clone());
            self.selected_idx = idx;
            Ok(name)
        }
        #[cfg(target_os = "linux")]
        {
            let midi_in = midir::MidiInput::new("axios24")
                .map_err(|e| format!("Erro MIDI ALSA: {}", e))?;
            let ports = midi_in.ports();
            let Some(port) = ports.get(idx) else {
                return Err("Porta MIDI não encontrada.".to_string());
            };
            let (tx, rx) = mpsc::channel();
            let conn = midi_in
                .connect(
                    port,
                    "axios24-in",
                    move |_stamp, msg, _| {
                        if !msg.is_empty() {
                            let _ = tx.send(MidiMsg {
                                status: msg[0],
                                data1: *msg.get(1).unwrap_or(&0),
                                data2: *msg.get(2).unwrap_or(&0),
                            });
                        }
                    },
                    (),
                )
                .map_err(|e| format!("Erro ao abrir porta MIDI '{}': {}", name, e))?;
            self.linux_conn = Some(conn);
            self.rx = Some(rx);
            self.connected_name = Some(name.clone());
            self.selected_idx = idx;
            Ok(name)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = idx;
            Err("MIDI USB nativo disponível no Windows (WinMM) e Linux (ALSA).".to_string())
        }
    }

    pub fn disconnect(&mut self) {
        #[cfg(windows)]
        unsafe {
            if self.handle != 0 {
                win_midi::midiInStop(self.handle);
                win_midi::midiInClose(self.handle);
                self.handle = 0;
            }
        }
        #[cfg(target_os = "linux")]
        {
            self.linux_conn = None;
        }
        self.rx = None;
        self.connected_name = None;
    }

    pub fn poll_messages(&mut self) -> Vec<MidiMsg> {
        let mut msgs = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                msgs.push(m);
            }
        }
        msgs
    }
}
