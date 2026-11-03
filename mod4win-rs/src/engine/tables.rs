//! Mathematische Lookup-Tabellen und Konstanten der MOD4WIN 2.40 Beta Audio-Engine.
//!
//! 1:1 extrahiert aus:
//! - `legacy_src/PLAYER/playdefs.pas` (`TuneTable`, `PitchTable`, `SinTable`, `RampUpTable`, `RampDnTable`, `SqrTable`, `HalfNote`, `HalfNote1`, `fPAL`, `fNTSC`, `DefMODTuning`)
//! - `legacy_src/PLAYER/player32.pas` (Tabellen-Initialisierung Zeilen 2711–2724 und kompilierte Binärdaten in `PLAYER32.DLL`)
//! - `legacy_src/PLAYER/play.inc` (`SetVolTable`, `VolAdjust1`, S3M `Scont`-Panning/Volume, `RetrigVolSlide`, Amiga-Panning)
//! - `legacy_src/PLAYER/loaders.pas` (STM `c2spd`-Korrekturtabelle, Nibble-Panning)
//! - `legacy_src/IT/AWE32DRV.ASM` (`AWEVolumeTable`)

// ============================================================================
// 1. Amiga-Taktraten, Basis-Tuning und Halbton-Faktoren (playdefs.pas)
// ============================================================================

/// PAL-Taktfrequenz des Amiga-Paula-Chips (`fPAL = 7093789.2` in `playdefs.pas` Zeile 881).
pub const F_PAL: f64 = 7_093_789.2;

/// NTSC-Taktfrequenz des Amiga-Paula-Chips (`fNTSC = 7159090.5` in `playdefs.pas` Zeile 882).
pub const F_NTSC: f64 = 7_159_090.5;

/// Standard-C2-Samplefrequenz für NTSC (`Trunc(fNTSC / 2 / 428) = 8363` Hz in `playdefs.pas` Zeile 883).
pub const DEF_MOD_TUNING_NTSC: u16 = 8363;

/// Alternative C2-Samplefrequenz für PAL (`Trunc(fPAL / 2 / 428) = 8287` Hz in `play.inc` Zeile 2263).
pub const DEF_MOD_TUNING_PAL: u16 = 8287;

/// 0.16-Fixed-Point-Multiplikator für einen Halbtonschritt nach oben (Periode verkleinern):
/// `HalfNote = $F1A2` (`61858`, entspricht `Trunc(65536 / 2^(1/12))`, `playdefs.pas` Zeile 884).
///
/// Genutzt in `effects.inc` (`DoVoiceHigher`, `DoArpeggio`, `DoArpeggio1`, `DoArpeggio2`).
pub const HALF_NOTE: u16 = 0xF1A2;

/// 16.16-Fixed-Point-Multiplikator für einen Halbtonschritt nach unten (Periode vergrößern):
/// `HalfNote1 = $10F39` (`69433`, entspricht `Round(65536 * 2^(1/12))`, `playdefs.pas` Zeile 885).
///
/// Genutzt in `effects.inc` (`DoVoiceLower`, `DoArpeggio1`, `DoArpeggio2`).
pub const HALF_NOTE_1: i32 = 0x0001_0F39;

/// Minimale zulässige Amiga-Periode (`14` in `effects.inc` und `play.inc`).
pub const MIN_AMIGA_PERIOD: u16 = 14;

/// Maximale zulässige Amiga-Periode (`54784` = `PitchTable[1]` in `effects.inc` und `play.inc`).
pub const MAX_AMIGA_PERIOD: u16 = 54784;

// ============================================================================
// 2. Finetune- und Amiga-Perioden-Tabellen (playdefs.pas, player32.pas)
// ============================================================================

/// 16.16-Fixed-Point-Finetune-Multiplikatoren für Finetune `0..15` (`0..+7`, `-8..-1`)
/// (`TuneTable: ARRAY[0..15] OF Longint` in `playdefs.pas` Zeilen 47–63).
///
/// Exakte 32-Bit-Werte aus dem kompilierten Datensegment von `PLAYER32.DLL` (Offset `127390`),
/// berechnet durch Borland Pascal 7.0 aus `Trunc(65536.0 * (436.4 / freq))`:
/// - Index `0..7`: Finetune `0..+7` (`436.4, 439.0, 441.6, 445.1, 447.8, 451.5, 455.2, 457.0` Hz)
/// - Index `8..15`: Finetune `-8..-1` (`412.0, 414.4, 416.7, 419.9, 423.1, 426.3, 429.6, 432.1` Hz)
pub const TUNE_TABLE: [i32; 16] = [
    65536, //  0: Trunc(65536.0 * (436.4 / 436.4)) = 0x00010000
    65147, // +1: Trunc(65536.0 * (436.4 / 439.0)) = 0x0000FE7B
    64764, // +2: Trunc(65536.0 * (436.4 / 441.6)) = 0x0000FCFC
    64255, // +3: Trunc(65536.0 * (436.4 / 445.1)) = 0x0000FAFF
    63867, // +4: Trunc(65536.0 * (436.4 / 447.8)) = 0x0000F97B
    63344, // +5: Trunc(65536.0 * (436.4 / 451.5)) = 0x0000F770
    62829, // +6: Trunc(65536.0 * (436.4 / 455.2)) = 0x0000F56D
    62581, // +7: Trunc(65536.0 * (436.4 / 457.0)) = 0x0000F475
    69417, // -8: Trunc(65536.0 * (436.4 / 412.0)) = 0x00010F29
    69015, // -7: Trunc(65536.0 * (436.4 / 414.4)) = 0x00010D97
    68634, // -6: Trunc(65536.0 * (436.4 / 416.7)) = 0x00010C1A
    68111, // -5: Trunc(65536.0 * (436.4 / 419.9)) = 0x00010A0F
    67596, // -4: Trunc(65536.0 * (436.4 / 423.1)) = 0x0001080C
    67088, // -3: Trunc(65536.0 * (436.4 / 426.3)) = 0x00010610
    66573, // -2: Trunc(65536.0 * (436.4 / 429.6)) = 0x0001040D
    66188, // -1: Trunc(65536.0 * (436.4 / 432.1)) = 0x0001028C
];

/// Amiga-Periodentabelle über 12 Oktaven (`PitchTable: ARRAY[0..145] OF Word` in `playdefs.pas` Zeile 42).
///
/// Initialisiert in `player32.pas` Zeilen 2711 & 2719:
/// - `PitchTable[0] = 0` (keine Note)
/// - `FOR i := 1 TO 145 DO PitchTable[i] := Round(54784.0 * Exp(ln(2) * ((1.0 - i) / 12)));`
///
/// Die Werte sind mit Faktor 8 skaliert (`54784 = 6848 * 8 = 856 * 64`).
/// Die klassische ProTracker-Oktave 1..3 (Perioden `856..113`) liegt mit Faktor 8 bei Index `37..72`
/// (`PitchTable[37] = 6848` = C-1, `PitchTable[49] = 3424` = C-2, `PitchTable[72] = 907` = B-3).
pub const PITCH_TABLE: [u16; 146] = [
        0, 54784, 51709, 48807, 46068, 43482, 41042, 38738, 36564, 34512, 32575, 30746,
    29021, 27392, 25855, 24403, 23034, 21741, 20521, 19369, 18282, 17256, 16287, 15373,
    14510, 13696, 12927, 12202, 11517, 10871, 10260,  9685,  9141,  8628,  8144,  7687,
     7255,  6848,  6464,  6101,  5758,  5435,  5130,  4842,  4570,  4314,  4072,  3843,
     3628,  3424,  3232,  3050,  2879,  2718,  2565,  2421,  2285,  2157,  2036,  1922,
     1814,  1712,  1616,  1525,  1440,  1359,  1283,  1211,  1143,  1078,  1018,   961,
      907,   856,   808,   763,   720,   679,   641,   605,   571,   539,   509,   480,
      453,   428,   404,   381,   360,   340,   321,   303,   286,   270,   254,   240,
      227,   214,   202,   191,   180,   170,   160,   151,   143,   135,   127,   120,
      113,   107,   101,    95,    90,    85,    80,    76,    71,    67,    64,    60,
       57,    53,    50,    48,    45,    42,    40,    38,    36,    34,    32,    30,
       28,    27,    25,    24,    22,    21,    20,    19,    18,    17,    16,    15,
       14,    13,
];

/// ScreamTracker 2 (`STM`) `c2spd`-Normalisierungstabelle (`loaders.pas` Zeilen 1665–1668).
///
/// Übersetzt historische STM-Finetune-Frequenzen in die von MOD4WIN erwarteten C2-Tuning-Werte.
pub const STM_C2SPD_TABLE: [(u16, u16); 15] = [
    (8448, 8424),
    (8519, 8463),
    (8608, 8529),
    (8753, 8581),
    (8893, 8651),
    (8968, 8723),
    (9043, 8757),
    (7680, 7895),
    (7765, 7941),
    (7850, 7985),
    (7954, 8046),
    (8058, 8107),
    (8162, 8169),
    (8264, 8232),
    (8363, 8280),
];

/// ScreamTracker 2 (`STM`) Tempo-Skalierungsfaktoren (`Fact` für `Op DIV 16 = 0..15`
/// in `effects.inc`: `DoSetSpeed`, Zeilen 720–729).
pub const STM_TEMPO_FACT_TABLE: [u8; 16] = [
    125, // 0
    100, 100, // 1..2
    50, 50, // 3..4
    25, 25, // 5..6
    12, 12, 12, 12, 12, 12, // 7..12
    6, 6, 6, // 13..15
];

// ============================================================================
// 3. Vibrato-, Tremolo- und Auto-Vibrato-Wellenformtabellen (playdefs.pas, player32.pas)
// ============================================================================

/// Kennung für Sinus-Wellenform (`SinTable`).
pub const WAVE_SINE: u16 = 0;
/// Kennung für fallende Sägezahn-Wellenform (`RampDnTable`).
pub const WAVE_RAMP_DN: u16 = 1;
/// Kennung für Rechteck-Wellenform (`SqrTable`).
pub const WAVE_SQUARE: u16 = 2;
/// Kennung für steigende Sägezahn-Wellenform (`RampUpTable`).
pub const WAVE_RAMP_UP: u16 = 3;

/// Sinus-Lookup-Tabelle für Vibrato, Tremolo und FT2-Auto-Vibrato
/// (`SinTable: ARRAY[0..255] OF Shortint` in `playdefs.pas` Zeile 847).
///
/// Initialisiert in `player32.pas` Zeile 2720:
/// `FOR i := 0 TO 255 DO sintable[i] := Trunc(-128 * Sin(Pi * i / 128) - 0.5);`
///
/// Hinweis: Durch das konstante `- 0.5` vor `Trunc` im Originalcode ist die erste Halbwelle (`1..127`)
/// negativ (`-3..-128`) und die zweite Halbwelle (`129..255`) positiv, aber um 1 Betragseinheit
/// kleiner (`2..127`), damit `+128` nicht den Wertebereich von `Shortint` (`i8`) sprengt.
pub const SIN_TABLE: [i8; 256] = [
       0,   -3,   -6,   -9,  -13,  -16,  -19,  -22,  -25,  -28,  -31,  -34,  -37,  -40,  -43,  -46,
     -49,  -52,  -55,  -58,  -60,  -63,  -66,  -68,  -71,  -74,  -76,  -79,  -81,  -84,  -86,  -88,
     -91,  -93,  -95,  -97,  -99, -101, -103, -105, -106, -108, -110, -111, -113, -114, -116, -117,
    -118, -119, -121, -122, -122, -123, -124, -125, -126, -126, -127, -127, -127, -128, -128, -128,
    -128, -128, -128, -128, -127, -127, -127, -126, -126, -125, -124, -123, -122, -122, -121, -119,
    -118, -117, -116, -114, -113, -111, -110, -108, -106, -105, -103, -101,  -99,  -97,  -95,  -93,
     -91,  -88,  -86,  -84,  -81,  -79,  -76,  -74,  -71,  -68,  -66,  -63,  -60,  -58,  -55,  -52,
     -49,  -46,  -43,  -40,  -37,  -34,  -31,  -28,  -25,  -22,  -19,  -16,  -13,   -9,   -6,   -3,
       0,    2,    5,    8,   12,   15,   18,   21,   24,   27,   30,   33,   36,   39,   42,   45,
      48,   51,   54,   57,   59,   62,   65,   67,   70,   73,   75,   78,   80,   83,   85,   87,
      90,   92,   94,   96,   98,  100,  102,  104,  105,  107,  109,  110,  112,  113,  115,  116,
     117,  118,  120,  121,  121,  122,  123,  124,  125,  125,  126,  126,  126,  127,  127,  127,
     127,  127,  127,  127,  126,  126,  126,  125,  125,  124,  123,  122,  121,  121,  120,  118,
     117,  116,  115,  113,  112,  110,  109,  107,  105,  104,  102,  100,   98,   96,   94,   92,
      90,   87,   85,   83,   80,   78,   75,   73,   70,   67,   65,   62,   59,   57,   54,   51,
      48,   45,   42,   39,   36,   33,   30,   27,   24,   21,   18,   15,   12,    8,    5,    2,
];

/// Fallende Rampen-/Sägezahn-Tabelle (`RampUpTable: ARRAY[0..255] OF Shortint` in `playdefs.pas` Zeile 847).
///
/// Initialisiert in `player32.pas` Zeile 2721:
/// `FOR i := 0 TO 255 DO RampUptable[i] := 127 - i;`
pub const RAMP_UP_TABLE: [i8; 256] = [
     127,  126,  125,  124,  123,  122,  121,  120,  119,  118,  117,  116,  115,  114,  113,  112,
     111,  110,  109,  108,  107,  106,  105,  104,  103,  102,  101,  100,   99,   98,   97,   96,
      95,   94,   93,   92,   91,   90,   89,   88,   87,   86,   85,   84,   83,   82,   81,   80,
      79,   78,   77,   76,   75,   74,   73,   72,   71,   70,   69,   68,   67,   66,   65,   64,
      63,   62,   61,   60,   59,   58,   57,   56,   55,   54,   53,   52,   51,   50,   49,   48,
      47,   46,   45,   44,   43,   42,   41,   40,   39,   38,   37,   36,   35,   34,   33,   32,
      31,   30,   29,   28,   27,   26,   25,   24,   23,   22,   21,   20,   19,   18,   17,   16,
      15,   14,   13,   12,   11,   10,    9,    8,    7,    6,    5,    4,    3,    2,    1,    0,
      -1,   -2,   -3,   -4,   -5,   -6,   -7,   -8,   -9,  -10,  -11,  -12,  -13,  -14,  -15,  -16,
     -17,  -18,  -19,  -20,  -21,  -22,  -23,  -24,  -25,  -26,  -27,  -28,  -29,  -30,  -31,  -32,
     -33,  -34,  -35,  -36,  -37,  -38,  -39,  -40,  -41,  -42,  -43,  -44,  -45,  -46,  -47,  -48,
     -49,  -50,  -51,  -52,  -53,  -54,  -55,  -56,  -57,  -58,  -59,  -60,  -61,  -62,  -63,  -64,
     -65,  -66,  -67,  -68,  -69,  -70,  -71,  -72,  -73,  -74,  -75,  -76,  -77,  -78,  -79,  -80,
     -81,  -82,  -83,  -84,  -85,  -86,  -87,  -88,  -89,  -90,  -91,  -92,  -93,  -94,  -95,  -96,
     -97,  -98,  -99, -100, -101, -102, -103, -104, -105, -106, -107, -108, -109, -110, -111, -112,
    -113, -114, -115, -116, -117, -118, -119, -120, -121, -122, -123, -124, -125, -126, -127, -128,
];

/// Steigende Rampen-/Sägezahn-Tabelle (`RampDnTable: ARRAY[0..255] OF Shortint` in `playdefs.pas` Zeile 847).
///
/// Initialisiert in `player32.pas` Zeile 2722:
/// `FOR i := 0 TO 255 DO RampDntable[i] := i - 128;`
pub const RAMP_DN_TABLE: [i8; 256] = [
    -128, -127, -126, -125, -124, -123, -122, -121, -120, -119, -118, -117, -116, -115, -114, -113,
    -112, -111, -110, -109, -108, -107, -106, -105, -104, -103, -102, -101, -100,  -99,  -98,  -97,
     -96,  -95,  -94,  -93,  -92,  -91,  -90,  -89,  -88,  -87,  -86,  -85,  -84,  -83,  -82,  -81,
     -80,  -79,  -78,  -77,  -76,  -75,  -74,  -73,  -72,  -71,  -70,  -69,  -68,  -67,  -66,  -65,
     -64,  -63,  -62,  -61,  -60,  -59,  -58,  -57,  -56,  -55,  -54,  -53,  -52,  -51,  -50,  -49,
     -48,  -47,  -46,  -45,  -44,  -43,  -42,  -41,  -40,  -39,  -38,  -37,  -36,  -35,  -34,  -33,
     -32,  -31,  -30,  -29,  -28,  -27,  -26,  -25,  -24,  -23,  -22,  -21,  -20,  -19,  -18,  -17,
     -16,  -15,  -14,  -13,  -12,  -11,  -10,   -9,   -8,   -7,   -6,   -5,   -4,   -3,   -2,   -1,
       0,    1,    2,    3,    4,    5,    6,    7,    8,    9,   10,   11,   12,   13,   14,   15,
      16,   17,   18,   19,   20,   21,   22,   23,   24,   25,   26,   27,   28,   29,   30,   31,
      32,   33,   34,   35,   36,   37,   38,   39,   40,   41,   42,   43,   44,   45,   46,   47,
      48,   49,   50,   51,   52,   53,   54,   55,   56,   57,   58,   59,   60,   61,   62,   63,
      64,   65,   66,   67,   68,   69,   70,   71,   72,   73,   74,   75,   76,   77,   78,   79,
      80,   81,   82,   83,   84,   85,   86,   87,   88,   89,   90,   91,   92,   93,   94,   95,
      96,   97,   98,   99,  100,  101,  102,  103,  104,  105,  106,  107,  108,  109,  110,  111,
     112,  113,  114,  115,  116,  117,  118,  119,  120,  121,  122,  123,  124,  125,  126,  127,
];

/// Rechteck-Wellenform-Tabelle (`SqrTable: ARRAY[0..255] OF Shortint` in `playdefs.pas` Zeile 847).
///
/// Initialisiert in `player32.pas` Zeilen 2723–2724:
/// - `FillChar(SqrTable[0], 128, -128);`
/// - `FillChar(SqrTable[128], 128, 127);`
pub const SQR_TABLE: [i8; 256] = [
    -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128,
    -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128,
    -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128,
    -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128,
    -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128,
    -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128,
    -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128,
    -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128, -128,
     127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,
     127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,
     127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,
     127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,
     127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,
     127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,
     127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,
     127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,  127,
];

// ============================================================================
// 4. Panning-Tabellen (loaders.pas, play.inc)
// ============================================================================

/// Standard-Amiga-LRRL-Panning (`Kanal MOD 4`: Links, Rechts, Rechts, Links) bei aktiviertem Panning
/// (`$30 = 48` für Kanal 0/3, `$C0 = 192` für Kanal 1/2 in `loaders.pas` Zeilen 881, 1409, 1732).
pub const AMIGA_PAN_TABLE_PANNED: [u8; 4] = [0x30, 0xC0, 0xC0, 0x30];

/// Hartes Amiga-LRRL-Panning (`0` = ganz links, `255` = ganz rechts), wenn `Panning == false`.
pub const AMIGA_PAN_TABLE_HARD: [u8; 4] = [0x00, 0xFF, 0xFF, 0x00];

/// 4-Bit-Nibble-zu-8-Bit-Panning-Tabelle (`(0..15) * 17` in `loaders.pas` Zeilen 1076, 1599, 1939
/// sowie `play.inc` Zeilen 1091, 1132 für Effekt `E8x` und FT2-Panning-Spalte `$Cx`).
pub const NIBBLE_PAN_TABLE: [u8; 16] = [
    0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255,
];

/// S3M Stereo-Control (`Scont - 1 = 0..7`) Panning-Lookup-Tabelle (`play.inc` Zeilen 190–199).
pub const S3M_SCONT_PAN_TABLE: [u8; 8] = [
    0,   // 0: Links (1/4 Vol)
    255, // 1: Rechts (1/4 Vol)
    0,   // 2: Links (1/2 Vol)
    255, // 3: Rechts (1/2 Vol)
    85,  // 4: Halblinks (3/4 Vol)
    170, // 5: Halbrechts (3/4 Vol)
    128, // 6: Mitte (1/2 Vol)
    128, // 7: Mitte (4/4 Vol)
];

/// S3M Stereo-Control (`Scont - 1 = 0..7`) Lautstärke-Zähler (`AvgVol = AvgVol * N / 4`, `play.inc` Zeilen 190–199).
pub const S3M_SCONT_VOL_NUM_TABLE: [u8; 8] = [1, 1, 2, 2, 3, 3, 2, 4];

/// Untergrenze des Mitten-Totbereichs beim Panning (`play.inc` Zeile 204: `IF (HB>=119)AND(HB<=136) THEN HB:=128`).
pub const PAN_CENTER_SNAP_MIN: u8 = 119;

/// Obergrenze des Mitten-Totbereichs beim Panning (`play.inc` Zeile 204).
pub const PAN_CENTER_SNAP_MAX: u8 = 136;

/// Exakte Mitten-Panning-Position (`128` in `play.inc` Zeile 204).
pub const PAN_CENTER: u8 = 128;

// ============================================================================
// 5. Volume-Rampen & Volume-Lookup-Tabellen (play.inc, AWE32DRV.ASM)
// ============================================================================

/// Additive Lautstärkeänderung für Effekt `RetrigVolSlide` (`Opnd SHR 4 = 0..15`, `play.inc` Zeilen 1257–1274).
///
/// Für Indizes `6, 7, 14, 15` ist der additive Wert `0`, da dort die multiplikativen Faktoren
/// aus [`RETRIG_VOL_MUL_TABLE`] und [`RETRIG_VOL_DIV_TABLE`] greifen.
pub const RETRIG_VOL_ADD_TABLE: [i8; 16] = [
    0, -1, -2, -4, -8, -16, 0, 0, 0, 1, 2, 4, 8, 16, 0, 0,
];

/// Multiplikator für Effekt `RetrigVolSlide` (`Opnd SHR 4 = 0..15`, `play.inc` Zeilen 1257–1274).
pub const RETRIG_VOL_MUL_TABLE: [u8; 16] = [
    1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 3, 2,
];

/// Divisor für Effekt `RetrigVolSlide` (`Opnd SHR 4 = 0..15`, `play.inc` Zeilen 1257–1274).
pub const RETRIG_VOL_DIV_TABLE: [u8; 16] = [
    1, 1, 1, 1, 1, 1, 3, 2, 1, 1, 1, 1, 1, 1, 2, 1,
];

/// Logarithmische Lautstärke-Dämpfungstabelle des Impulse-Tracker AWE32-Treibers
/// (`AWEVolumeTable` in `legacy_src/IT/AWE32DRV.ASM` Zeilen 230–250, `-log(vol/256) * (160/3)`).
pub const AWE_VOLUME_TABLE: [u8; 129] = [
    255, 128, 112, 103,  96,  91,  87,  83, // 0..7
     80,  78,  75,  73,  71,  69,  67,  66, // 8..15
     64,  63,  61,  60,  59,  58,  57,  56, // 16..23
     55,  54,  53,  52,  51,  50,  50,  49, // 24..31
     48,  47,  47,  46,  45,  45,  44,  44, // 32..39
     43,  42,  42,  41,  41,  40,  40,  39, // 40..47
     39,  38,  38,  37,  37,  36,  36,  36, // 48..55
     35,  35,  34,  34,  34,  33,  33,  32, // 56..63
     32,  32,  31,  31,  31,  30,  30,  30, // 64..71
     29,  29,  29,  28,  28,  28,  28,  27, // 72..79
     27,  27,  26,  26,  26,  26,  25,  25, // 80..87
     25,  25,  24,  24,  24,  23,  23,  23, // 88..95
     23,  22,  22,  22,  22,  22,  21,  21, // 96..103
     21,  21,  20,  20,  20,  20,  20,  19, // 104..111
     19,  19,  19,  19,  18,  18,  18,  18, // 112..119
     18,  17,  17,  17,  17,  17,  16,  16, // 120..127
     16,                                     // 128
];

/// Berechnet die 65x256 (16-Bit) Volume-Rampen-/Lookup-Tabelle des 8-Bit-Assembler-Mixers
/// bit-identisch zu `SetVolTable` in `legacy_src/PLAYER/play.inc` (Zeilen 2202–2244).
///
/// Layout im Original-Assembler:
/// - Äußerer Index `vol` (`esi = 0..=64`): Kanallautstärke `lVol`, `rVol` bzw. `avgVol` (im Mixer in Register `BH`).
/// - Innerer Index `raw_byte` (`bl = cl as u8`, für `ecx = -128..=127`): das direkt aus dem Speicher gelesene
///   8-Bit-Sample-Byte (`0x00..=0x7F` entspricht `0..=127`, `0x80..=0xFF` entspricht `-128..=-1`).
/// - Formel: `((sample_i8 as i32 * vol as i32 * vol_master as i32) >> 9).clamp(-32768, 32767) as i16`.
pub const fn build_vol_table(vol_master: u16) -> [[i16; 256]; 65] {
    let mut table = [[0i16; 256]; 65];
    let edx = vol_master as i32;
    let mut esi: i32 = 0;
    while esi < 65 {
        let mut ecx: i32 = -128;
        while ecx < 128 {
            let mut eax = ecx * esi * edx;
            eax >>= 9; // sar eax, 9
            if eax < -32768 {
                eax = -32768;
            } else if eax > 32767 {
                eax = 32767;
            }
            let bx = (ecx as u8) as usize;
            table[esi as usize][bx] = eax as i16;
            ecx += 1;
        }
        esi += 1;
    }
    table
}

/// Standard-Volume-Lookup-Tabelle für `VolMaster = 512` (1:1 Einheitsverstärkung `512 >> 9 = 1`,
/// z. B. 4 Kanäle bei `0 dB` PreAmp: `MV = 256 * 8 / 4 = 512`).
pub const DEFAULT_VOL_TABLE_512: [[i16; 256]; 65] = build_vol_table(512);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tables_integrity() {
        assert_eq!(PITCH_TABLE[0], 0);
        assert_eq!(PITCH_TABLE[1], 54784);
        assert_eq!(PITCH_TABLE[37], 6848); // 856 * 8 (C-1)
        assert_eq!(PITCH_TABLE[49], 3424); // 428 * 8 (C-2)
        assert_eq!(PITCH_TABLE[121], 53);
        assert_eq!(PITCH_TABLE[145], 13);

        assert_eq!(TUNE_TABLE[0], 65536);
        assert_eq!(TUNE_TABLE[7], 62581);
        assert_eq!(TUNE_TABLE[8], 69417);
        assert_eq!(TUNE_TABLE[15], 66188);

        assert_eq!(SIN_TABLE[0], 0);
        assert_eq!(SIN_TABLE[64], -128);
        assert_eq!(SIN_TABLE[128], 0);
        assert_eq!(SIN_TABLE[192], 127);

        for i in 0..256 {
            assert_eq!(RAMP_UP_TABLE[i], (127 - i as i16) as i8);
            assert_eq!(RAMP_DN_TABLE[i], (i as i16 - 128) as i8);
            if i < 128 {
                assert_eq!(SQR_TABLE[i], -128);
            } else {
                assert_eq!(SQR_TABLE[i], 127);
            }
        }

        // Prüfe SetVolTable bei VolMaster = 512: sample * vol * 512 >> 9 == sample * vol
        assert_eq!(DEFAULT_VOL_TABLE_512[64][0x80], -128 * 64); // -128 * 64 = -8192
        assert_eq!(DEFAULT_VOL_TABLE_512[64][0x7F], 127 * 64); // 127 * 64 = 8128
        assert_eq!(DEFAULT_VOL_TABLE_512[0][0x80], 0);
    }
}

