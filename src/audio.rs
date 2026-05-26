use std::io::{self, Write};
use std::time::Duration;

use rodio::source::{SineWave, Source};
use rodio::{DeviceSinkBuilder, MixerDeviceSink};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeySoundStyle {
    Off,
    Click,
    Beep,
    Thock,
    Crisp,
    Mechanical,
}

#[derive(Debug, Clone, Copy)]
struct Tone {
    frequency: f32,
    duration_ms: u64,
    fade_ms: u64,
    volume: f32,
}

pub struct KeyClickPlayer {
    sink: Option<MixerDeviceSink>,
    enabled: bool,
    style: KeySoundStyle,
}

impl KeyClickPlayer {
    pub fn new(style: &str) -> Self {
        let style = KeySoundStyle::from_config(style);
        Self {
            sink: open_sink(style != KeySoundStyle::Off),
            enabled: style != KeySoundStyle::Off,
            style,
        }
    }

    pub fn set_style(&mut self, style: &str) {
        let style = KeySoundStyle::from_config(style);
        let enabled = style != KeySoundStyle::Off;
        if self.enabled == enabled && self.style == style {
            return;
        }

        let reopen_sink = self.enabled != enabled;
        self.enabled = enabled;
        self.style = style;
        if reopen_sink {
            self.sink = open_sink(enabled);
        }
    }

    pub fn play_key(&self) {
        self.play_tones(key_tones(self.style));
    }

    pub fn play_mouse(&self) {
        self.play_tones(mouse_tones(self.style));
    }

    fn play_tones(&self, tones: &[Tone]) {
        if !self.enabled {
            return;
        }

        let Some(sink) = &self.sink else {
            play_terminal_bell();
            return;
        };

        for tone in tones {
            let click = SineWave::new(tone.frequency)
                .take_duration(Duration::from_millis(tone.duration_ms))
                .fade_out(Duration::from_millis(tone.fade_ms))
                .amplify(tone.volume);
            sink.mixer().add(click);
        }
    }
}

impl KeySoundStyle {
    fn from_config(value: &str) -> Self {
        match value {
            "off" => Self::Off,
            "beep" => Self::Beep,
            "thock" => Self::Thock,
            "crisp" => Self::Crisp,
            "mechanical" => Self::Mechanical,
            _ => Self::Click,
        }
    }
}

fn open_sink(enabled: bool) -> Option<MixerDeviceSink> {
    if !enabled {
        return None;
    }

    DeviceSinkBuilder::open_default_sink()
        .map(|mut sink| {
            sink.log_on_drop(false);
            sink
        })
        .ok()
}

fn play_terminal_bell() {
    let mut stdout = io::stdout();
    let _ = stdout.write_all(b"\x07");
    let _ = stdout.flush();
}

fn key_tones(style: KeySoundStyle) -> &'static [Tone] {
    match style {
        KeySoundStyle::Off => &[],
        KeySoundStyle::Click => &[Tone {
            frequency: 1_750.0,
            duration_ms: 16,
            fade_ms: 11,
            volume: 0.075,
        }],
        KeySoundStyle::Beep => &[Tone {
            frequency: 1_050.0,
            duration_ms: 26,
            fade_ms: 18,
            volume: 0.055,
        }],
        KeySoundStyle::Thock => &[
            Tone {
                frequency: 260.0,
                duration_ms: 34,
                fade_ms: 26,
                volume: 0.105,
            },
            Tone {
                frequency: 720.0,
                duration_ms: 14,
                fade_ms: 10,
                volume: 0.035,
            },
        ],
        KeySoundStyle::Crisp => &[
            Tone {
                frequency: 2_450.0,
                duration_ms: 10,
                fade_ms: 7,
                volume: 0.070,
            },
            Tone {
                frequency: 4_250.0,
                duration_ms: 5,
                fade_ms: 4,
                volume: 0.028,
            },
        ],
        KeySoundStyle::Mechanical => &[
            Tone {
                frequency: 310.0,
                duration_ms: 28,
                fade_ms: 22,
                volume: 0.090,
            },
            Tone {
                frequency: 1_650.0,
                duration_ms: 12,
                fade_ms: 8,
                volume: 0.055,
            },
            Tone {
                frequency: 3_700.0,
                duration_ms: 5,
                fade_ms: 4,
                volume: 0.030,
            },
        ],
    }
}

fn mouse_tones(style: KeySoundStyle) -> &'static [Tone] {
    match style {
        KeySoundStyle::Off => &[],
        KeySoundStyle::Click => &[Tone {
            frequency: 980.0,
            duration_ms: 20,
            fade_ms: 14,
            volume: 0.080,
        }],
        KeySoundStyle::Beep => &[Tone {
            frequency: 760.0,
            duration_ms: 30,
            fade_ms: 22,
            volume: 0.055,
        }],
        KeySoundStyle::Thock => &[Tone {
            frequency: 210.0,
            duration_ms: 38,
            fade_ms: 30,
            volume: 0.110,
        }],
        KeySoundStyle::Crisp => &[
            Tone {
                frequency: 1_950.0,
                duration_ms: 13,
                fade_ms: 9,
                volume: 0.060,
            },
            Tone {
                frequency: 3_200.0,
                duration_ms: 6,
                fade_ms: 5,
                volume: 0.026,
            },
        ],
        KeySoundStyle::Mechanical => &[
            Tone {
                frequency: 250.0,
                duration_ms: 30,
                fade_ms: 24,
                volume: 0.095,
            },
            Tone {
                frequency: 1_250.0,
                duration_ms: 14,
                fade_ms: 10,
                volume: 0.045,
            },
        ],
    }
}
