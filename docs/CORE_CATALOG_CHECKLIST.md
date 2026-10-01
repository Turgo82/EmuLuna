# EmuLuna and Libretro core checklist

Snapshot: buildbot availability checked 2026-09-28; curated catalog updated 2026-09-30 · Linux x86_64 · [official Libretro buildbot](https://buildbot.libretro.com/nightly/linux/x86_64/latest/)

- **241** downloadable Libretro core packages in the current buildbot index
- **29** cores in EmuLuna’s curated download catalog
- **13** curated cores installed in the active library
- **212** downloadable packages outside EmuLuna’s catalog at the snapshot date

Checklist meaning: checked items are included in EmuLuna’s curated downloader. Unchecked items exist on the official buildbot but are not currently offered by EmuLuna. “Installed” reflects the active library at the time this report was generated.

The buildbot includes more than console emulators. It also contains arcade variants, computer emulators, standalone games, game engines, media players, utilities, and experimental cores. Adding a core to EmuLuna may also require a new system definition, file detection, BIOS rules, controls, controller artwork, save handling, and launch testing.

## EmuLuna curated catalog

- [x] **Gambatte** (`gambatte`) — Game Boy, Game Boy Color — Not installed — build 2026-09-28
- [x] **mGBA** (`mgba`) — Game Boy, Game Boy Color, Game Boy Advance — **Installed:** 0.11-212-7a12d6d — build 2026-09-28
- [x] **Snes9x** (`snes9x`) — Super Nintendo (SNES) — **Installed:** 1.63 fae2fea — build 2026-09-28
- [x] **Nestopia** (`nestopia`) — Nintendo (NES), Famicom Disk System — **Installed:** 2.0.0 92578fd — build 2026-09-28
- [x] **Stella** (`stella`) — Atari 2600 — **Installed:** 8.0_pre e86e1f6 — build 2026-09-28
- [x] **a5200** (`a5200`) — Atari 5200 — Not installed — build 2026-09-28
- [x] **ProSystem** (`prosystem`) — Atari 7800 — Not installed — build 2026-09-28
- [x] **Handy** (`handy`) — Atari Lynx — Not installed — build 2026-09-28
- [x] **Gearcoleco** (`gearcoleco`) — ColecoVision — Not installed — build 2026-09-28
- [x] **FreeIntv** (`freeintv`) — Intellivision — Not installed — build 2026-09-28
- [x] **Genesis Plus GX** (`genesis_plus_gx`) — Game Gear, Sega Mega Drive, Sega Master System, SG-1000, Sega Mega-CD — **Installed:** v1.7.4 c2838c7 — build 2026-09-28
- [x] **PicoDrive** (`picodrive`) — Sega Mega Drive, Sega Master System, Sega 32X, Sega Mega-CD — Not installed — build 2026-09-28
- [x] **Beetle Saturn** (`mednafen_saturn`) — Sega Saturn — **Installed:** v1.32.1 — build 2026-09-28
- [x] **Beetle NeoPop** (`mednafen_ngp`) — NeoGeo Pocket, Neo Geo Pocket Color — Not installed — build 2026-09-28
- [x] **ParaLLEl N64** (`parallel_n64`) — Nintendo 64 — **Installed:** 1.0 6e4c44c — build 2026-09-28
- [x] **DeSmuME** (`desmume`) — Nintendo DS — Not installed — build 2026-09-28
- [x] **O2EM** (`o2em`) — Odyssey² — Not installed — build 2026-09-28
- [x] **Beetle PCE FAST** (`mednafen_pce_fast`) — TurboGrafx-16, TurboGrafx-CD — **Installed:** v1.31.0.0 — build 2026-09-28
- [x] **PCSX ReARMed** (`pcsx_rearmed`) — Sony PlayStation — **Installed:** r26 ff81ed1 — build 2026-09-28
- [x] **PPSSPP** (`ppsspp`) — Sony PSP — Not installed — build 2026-09-28
- [x] **vecx** (`vecx`) — Vectrex — **Installed:** 1.2 8f671cc — build 2026-09-28
- [x] **Beetle VB** (`mednafen_vb`) — Virtual Boy — **Installed:** v1.31.0 83ed426 — build 2026-09-28
- [x] **Beetle Wonderswan** (`mednafen_wswan`) — WonderSwan, WonderSwan Color — Not installed — build 2026-09-28
- [x] **SameBoy** (`sameboy`) — Game Boy, Game Boy Color — Not installed — build 2026-09-28
- [x] **Snes9x 2010** (`snes9x2010`) — Super Nintendo (SNES) — Not installed — build 2026-09-28
- [x] **bsnes** (`bsnes`) — Super Nintendo (SNES) — **Installed:** 115 — build 2026-09-28
- [x] **FCEUmm** (`fceumm`) — Nintendo (NES), Famicom Disk System — **Installed:** (SVN) 236ccdf — build 2026-09-28

## Available from Libretro but outside EmuLuna’s catalog

### Emulator (168)

- [ ] **Sinclair - ZX 81 (EightyOne)** (`81`) — ZX81 — build 2026-09-28
- [ ] **Arcadia 2001 / Interton VC 4000 (AmiArcadia)** (`amiarcadia`) — Arcadia 2001 — build 2026-09-27
- [ ] **Commodore - Amiga (Amiberry)** (`amiberry`) — Amiga — build 2026-09-28
- [ ] **Apple II (AppleWin)** (`applewin`) — II — build 2026-09-28
- [ ] **Arduboy (Ardens)** (`ardens`) — Arduboy — build 2026-09-28
- [ ] **Arduboy (Arduous)** (`arduous`) — Arduboy — build 2026-09-28
- [ ] **Atari - 400/800/600XL/800XL/130XE/5200 (Atari800)** (`atari800`) — Atari 8-bit Family — build 2026-09-28
- [ ] **Nintendo - 3DS (Azahar)** (`azahar`) — 3DS — build 2026-09-28
- [ ] **Acorn - BBC Micro (b2)** (`b2`) — BBC Micro — build 2026-09-28
- [ ] **BBKEmu (BBK Electronic Dictionary)** (`bbkemu`) — BBK Electronic Dictionary — build 2026-09-28
- [ ] **Sega - MS/GG/MD/CD/32X (BlastEm)** (`blastem`) — Sega 8/16-bit (Various) — build 2026-09-28
- [ ] **MSX/SVI/ColecoVision/SG-1000 (blueMSX)** (`bluemsx`) — MSX/SVI/ColecoVision/SG-1000 — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (bsnes-jg)** (`bsnes-jg`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (bsnes 2014 Accuracy)** (`bsnes2014_accuracy`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (bsnes 2014 Balanced)** (`bsnes2014_balanced`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (bsnes 2014 Performance)** (`bsnes2014_performance`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (bsnes C++98 (v085))** (`bsnes_cplusplus98`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (bsnes-hd beta)** (`bsnes_hd_beta`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (bsnes-mercury Accuracy)** (`bsnes_mercury_accuracy`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (bsnes-mercury Balanced)** (`bsnes_mercury_balanced`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (bsnes-mercury Performance)** (`bsnes_mercury_performance`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Amstrad - CPC/GX4000 (Caprice32)** (`cap32`) — CPC/GX4000 — build 2026-09-28
- [ ] **Philips CDi (CDi 2015)** (`cdi2015`) — CD-i — build 2026-09-28
- [ ] **Nintendo - Wii U (Cemu)** (`cemu`) — Wii U — build 2026-09-28
- [ ] **Nintendo - 3DS (Citra)** (`citra`) — 3DS — build 2026-09-28
- [ ] **Nintendo - 3DS (Citra 2018)** (`citra2018`) — 3DS — build 2026-09-28
- [ ] **Sega - MD/CD (ClownMDEmu)** (`clownmdemu`) — Sega Genesis — build 2026-09-28
- [ ] **Amstrad - CPC (CrocoDS)** (`crocods`) — CPC — build 2026-09-28
- [ ] **Nintendo - DS (DeSmuME 2015)** (`desmume2015`) — Nintendo DS — build 2026-09-28
- [ ] **Arcade (DICE)** (`dice`) — Arcade (various) — build 2026-09-28
- [ ] **Dingoo A320 / Gemei A330 (DingooEmu)** (`dingooemu`) — Dingoo A320 — build 2026-09-28
- [ ] **Laserdisc arcade game (DirkSimple)** (`dirksimple`) — Laserdisc arcade game — build 2026-09-28
- [ ] **Nintendo - GameCube / Wii (Dolphin)** (`dolphin`) — GameCube / Wii — build 2026-09-28
- [ ] **DOS (DOSBox)** (`dosbox`) — DOS — build 2026-09-28
- [ ] **DOS (DOSBox-core)** (`dosbox_core`) — DOS — build 2026-09-28
- [ ] **DOS (DOSBox-Pure)** (`dosbox_pure`) — DOS — build 2026-09-28
- [ ] **DOS (DOSBox-SVN)** (`dosbox_svn`) — DOS — build 2026-09-28
- [ ] **Nintendo - Game Boy / Color (DoubleCherryGB)** (`DoubleCherryGB`) — Game Boy/Game Boy Color — build 2026-09-28
- [ ] **EPOCH/YENO Super Cassette Vision (EmuSCV)** (`emuscv`) — Super Cassette Vision — build 2026-09-28
- [ ] **Enterprise - 64/128 (ep128emu)** (`ep128emu_core`) — 128 — build 2026-09-28
- [ ] **Arcade (FB Alpha 2012)** (`fbalpha2012`) — Arcade (various) — build 2026-09-28
- [ ] **Arcade (FB Alpha 2012 CPS-1)** (`fbalpha2012_cps1`) — CP System I — build 2026-09-28
- [ ] **Arcade (FB Alpha 2012 CPS-2)** (`fbalpha2012_cps2`) — CP System II — build 2026-09-28
- [ ] **Arcade (FB Alpha 2012 CPS-3)** (`fbalpha2012_cps3`) — CP System III — build 2026-09-28
- [ ] **Arcade (FB Alpha 2012 Neo Geo)** (`fbalpha2012_neogeo`) — Neo Geo — build 2026-09-28
- [ ] **Arcade (FinalBurn Neo)** (`fbneo`) — Arcade (various) — build 2026-09-28
- [ ] **Nintendo - Game Boy / Color (fixGB)** (`fixgb`) — Game Boy/Game Boy Color — build 2026-09-28
- [ ] **Nintendo - NES / Famicom (fixNES)** (`fixnes`) — Nintendo Entertainment System — build 2026-09-28
- [ ] **Sega - Dreamcast/Naomi (Flycast)** (`flycast`) — Sega Dreamcast — build 2026-09-28
- [ ] **Microsoft - MSX (fMSX)** (`fmsx`) — MSX — build 2026-09-28
- [ ] **Fairchild - ChannelF (FreeChaF)** (`freechaf`) — Channel F — build 2026-09-28
- [ ] **Commodore - C64 (Frodo)** (`frodo`) — C64 — build 2026-09-28
- [ ] **Commodore - Amiga (FS-UAE)** (`fsuae`) — Amiga — build 2026-09-28
- [ ] **Sinclair - ZX Spectrum (Fuse)** (`fuse`) — ZX Spectrum (various) — build 2026-09-28
- [ ] **Elektronika Inženjering - Galaksija (Galaksija)** (`galaksija`) — Galaksija — build 2026-09-28
- [ ] **GAM4980** (`gam4980`) — Longman 4980 — build 2026-09-27
- [ ] **Nintendo - Game Boy / Color (Gearboy)** (`gearboy`) — Game Boy/Game Boy Color — build 2026-09-28
- [ ] **NEC - PC Engine / SuperGrafx / CD (Geargrafx)** (`geargrafx`) — PC Engine/SuperGrafx — build 2026-09-28
- [ ] **Atari - Lynx (Gearlynx)** (`gearlynx`) — Lynx — build 2026-09-28
- [ ] **Sega - MS/GG/SG-1000 (Gearsystem)** (`gearsystem`) — Sega 8-bit (MS/GG/SG-1000) — build 2026-09-28
- [ ] **Sega - MS/GG/MD/CD (Genesis Plus GX Wide)** (`genesis_plus_gx_wide`) — Sega 8/16-bit (Various) — build 2026-09-28
- [ ] **SNK - Neo Geo AES/MVS/CD (Geolith)** (`geolith`) — Neo Geo — build 2026-09-28
- [ ] **Nintendo - Game Boy Advance (gpSP)** (`gpsp`) — Game Boy Advance — build 2026-09-28
- [ ] **Handheld Electronic (GW)** (`gw`) — Handheld Electronic — build 2026-09-28
- [ ] **Atari - ST / STE / TT / Falcon (Hatari)** (`hatari`) — Atari ST/STE/TT/Falcon — build 2026-09-28
- [ ] **Atari - ST / STE / TT / Falcon (Hatari 2014)** (`hatari2014`) — Atari ST/STE/TT/Falcon — build 2026-09-27
- [ ] **Atari - ST/STE/TT/Falcon (hatariB)** (`hatarib`) — Atari ST/STE/TT/Falcon — build 2026-09-28
- [ ] **Arcade (HBMAME)** (`hbmame`) — Arcade (various) — build 2026-09-28
- [ ] **Atari - Lynx (Holani)** (`holani`) — Lynx — build 2026-09-28
- [ ] **Nintendo - Game Boy / Color (IroGB)** (`irogb`) — Game Boy/Game Boy Color — build 2026-09-28
- [ ] **CHIP-8/S-CHIP/XO-CHIP (JAXE)** (`jaxe`) — CHIP-8 — build 2026-09-28
- [ ] **ColecoVision/CreatiVision/My Vision (JollyCV)** (`jollycv`) — ColecoVision/CreatiVision/My Vision — build 2026-09-28
- [ ] **Sega - Saturn/ST-V (Kronos)** (`kronos`) — Saturn — build 2026-09-28
- [ ] **Philips - P2000T (M2000)** (`m2000`) — P2000T — build 2026-09-28
- [ ] **Arcade (MAME)** (`mame`) — Arcade (various) — build 2026-09-28
- [ ] **Arcade (MAME 2000)** (`mame2000`) — Arcade (various) — build 2026-09-28
- [ ] **Arcade (MAME 2003)** (`mame2003`) — Arcade (various) — build 2026-09-28
- [ ] **Arcade (MAME 2003 Midway)** (`mame2003_midway`) — Arcade (various) — build 2026-09-28
- [ ] **Arcade (MAME 2003-Plus)** (`mame2003_plus`) — Arcade (various) — build 2026-09-28
- [ ] **Arcade (MAME 2010)** (`mame2010`) — Arcade (various) — build 2026-09-28
- [ ] **Arcade (MAME 2015)** (`mame2015`) — Arcade (various) — build 2026-09-28
- [ ] **Arcade (MAME 2016)** (`mame2016`) — Arcade (various) — build 2026-09-27
- [ ] **McSoftServe** (`mcsoftserve`) — C713 — build 2024-04-01
- [ ] **Nintendo - Game Boy Advance (Beetle GBA)** (`mednafen_gba`) — Game Boy Advance — build 2026-09-28
- [ ] **Atari - Lynx (Beetle Lynx)** (`mednafen_lynx`) — Lynx — build 2026-09-28
- [ ] **NEC - PC Engine / SuperGrafx / CD (Beetle PCE)** (`mednafen_pce`) — PC Engine/SuperGrafx/CD — build 2026-09-28
- [ ] **NEC - PC-FX (Beetle PC-FX)** (`mednafen_pcfx`) — PC-FX — build 2026-09-28
- [x] **Sony - PlayStation (Beetle PSX)** (`mednafen_psx`) — PlayStation — available to download
- [x] **Sony - PlayStation (Beetle PSX HW)** (`mednafen_psx_hw`) — PlayStation — available to download
- [ ] **Nintendo - SNES / SFC (Beetle bsnes)** (`mednafen_snes`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (Beetle Supafaust)** (`mednafen_supafaust`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **NEC - PC Engine SuperGrafx (Beetle SuperGrafx)** (`mednafen_supergrafx`) — PC Engine SuperGrafx — build 2026-09-28
- [ ] **Nintendo - DS (melonDS)** (`melonds`) — Nintendo DS — build 2026-09-28
- [ ] **Nintendo - DS (melonDS DS)** (`melondsds`) — Nintendo DS — build 2026-09-28
- [ ] **Nintendo - NES / Famicom (Mesen)** (`mesen`) — Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC / Game Boy / Color (Mesen-S)** (`mesen-s`) — Super Nintendo Entertainment System / Game Boy / Game Boy Color — build 2026-09-28
- [ ] **Nintendo - NES / FC / FDS / SNES / SFC / GB / GBC / GBA / NEC - PCE / PCE-CD / Sega - SMS / GG / Bandai - Wswan (MesenCE)** (`mesen2`) — Nintendo Entertainment System / Super Nintendo Entertainment System / Game Boy / Game Boy Color / PC Engine SuperGrafx / PC Engine - TurboGrafx 16 / PC Engine CD - TurboGrafx-CD / WonderSwan / WonderSwan Color — build 2026-09-28
- [ ] **Nintendo - Game Boy Advance (Meteor)** (`meteor`) — Game Boy Advance — build 2026-09-28
- [ ] **Mac II (minivmac)** (`minivmac`) — Mac68k — build 2026-09-28
- [ ] **Infocom Z-Machine (MojoZork)** (`mojozork`) — Z-Machine — build 2026-09-28
- [ ] **Palm OS (Mu)** (`mu`) — Palm OS — build 2026-09-28
- [ ] **Nintendo - Nintendo 64 (Mupen64Plus-Next)** (`mupen64plus_next`) — Nintendo 64 — build 2026-09-19
- [ ] **Native32 (Native32Emu)** (`native32emu`) — Native32 — build 2026-09-27
- [ ] **NEC - PC-98 (Neko Project II)** (`nekop2`) — PC-98 — build 2026-09-28
- [ ] **SNK - Neo Geo CD (NeoCD)** (`neocd`) — SNK Neo Geo CD — build 2026-09-28
- [ ] **Nicai (NicaiEmu)** (`nicaiemu`) — Nicai — build 2026-09-28
- [ ] **Nintendo - DS (NooDS)** (`noods`) — Nintendo DS — build 2026-09-28
- [ ] **NEC - PC-98 (Neko Project II Kai)** (`np2kai`) — PC-98 — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (nSide Balanced)** (`nside_sfc_balanced`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **VM Labs - NUON (Nuance)** (`nuance`) — NUON — build 2026-09-28
- [ ] **Texas Instruments TI-83 (Numero)** (`numero`) — TI83 — build 2026-09-28
- [ ] **Oberon RISC Emulator** (`oberon`) — Oberon RISC machine — build 2026-09-28
- [ ] **The 3DO Company - 3DO (Opera)** (`opera`) — 3DO — build 2026-09-28
- [ ] **Nintendo - 3DS (Panda3DS)** (`panda3ds`) — 3DS — build 2026-09-19
- [ ] **Sony - PlayStation 2 (PCEE2)** (`pcee2`) — Sony PlayStation 2 — build 2026-08-21
- [ ] **PC (PCem)** (`pcem`) — PC — build 2026-09-28
- [ ] **Sony - PlayStation 2 (LRPS2)** (`pcsx2`) — Sony PlayStation 2 — build 2026-09-28
- [ ] **Epoch - Cassette Vision (PD777)** (`pd777`) — Epoch Cassette Vision — build 2026-09-27
- [ ] **Sony - PlayStation 2 (Play!)** (`play`) — Sony PlayStation 2 — build 2026-09-28
- [ ] **Playdia (PlaydiaEmu)** (`playdiaemu`) — Playdia — build 2026-09-28
- [ ] **Nintendo - Pokemon Mini (PokeMini)** (`pokemini`) — Pokemon Mini — build 2026-09-28
- [ ] **Sony - PocketStation (pokketstation)** (`pokketstation`) — PocketStation — build 2026-09-28
- [ ] **Watara - Supervision (Potator)** (`potator`) — Supervision — build 2026-09-28
- [ ] **Commodore - Amiga (PUAE)** (`puae`) — Amiga — build 2026-09-28
- [ ] **Commodore - Amiga (PUAE 2021)** (`puae2021`) — Amiga — build 2026-09-28
- [ ] **Sharp - X68000 (PX68k)** (`px68k`) — Sharp X68000 — build 2026-09-28
- [ ] **QEMU** (`qemu`) — Unspecified — build 2026-07-27
- [ ] **NEC - PC-88 series (QUASI88)** (`quasi88`) — PC-88 series — build 2026-09-28
- [ ] **Nintendo - NES / Famicom (QuickNES)** (`quicknes`) — Nintendo Entertainment System — build 2026-09-28
- [ ] **SNK - Neo Geo Pocket / Color (RACE)** (`race`) — Neo Geo Pocket (Color) — build 2026-09-28
- [ ] **PICO-8 (Retro8)** (`retro8`) — PICO-8 — build 2026-09-28
- [ ] **Nintendo - NES / Famicom (RustyNES)** (`rustynes`) — Nintendo Entertainment System — build 2026-09-29
- [ ] **Philips - CDi (SAME CDi)** (`same_cdi`) — CD-i — build 2026-09-28
- [ ] **Mega Duck / Cougar Boy (SameDuck)** (`sameduck`) — Mega Duck — build 2026-09-28
- [ ] **Nintendo - Game Boy/GBA/NDS (SkyEmu)** (`skyemu`) — Unspecified — build 2026-09-28
- [ ] **Sega - MS/GG (SMS Plus GX)** (`smsplus`) — Sega 8-bit — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (Snes9x 2002)** (`snes9x2002`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (Snes9x 2005)** (`snes9x2005`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **Nintendo - SNES / SFC (Snes9x 2005 Plus)** (`snes9x2005_plus`) — Super Nintendo Entertainment System — build 2026-09-28
- [ ] **SPMP8000 (SPMP8000Emu)** (`spmp8000emu`) — SPMP8000 — build 2026-09-28
- [ ] **Java ME (SquirrelJME)** (`squirreljme`) — Java ME — build 2026-09-28
- [ ] **Atari - 2600 (Stella 2014)** (`stella2014`) — Atari 2600 — build 2026-09-28
- [ ] **Atari - 2600 (Stella 2023)** (`stella2023`) — Atari 2600 — build 2026-09-28
- [ ] **Sega - Model 3 (Supermodel)** (`supermodel`) — Model 3 — build 2026-09-28
- [ ] **Sony - PlayStation (SwanStation)** (`swanstation`) — PlayStation — build 2026-09-27
- [ ] **Bandai - Tamagotchi P1 (TamaLIBretro)** (`tamalibretro`) — Tamagotchi P1 — build 2026-09-28
- [ ] **Nintendo - Game Boy / Color (TGB Dual)** (`tgbdual`) — Game Boy/Game Boy Color — build 2026-09-28
- [ ] **Thomson - MO/TO (Theodore)** (`theodore`) — Thomson MO/TO — build 2026-09-28
- [ ] **Atari - 2600 (Tia)** (`tia`) — Atari 2600 — build 2026-09-28
- [ ] **Uzebox (Uzem)** (`uzem`) — Uzebox — build 2026-09-28
- [ ] **VaporSpec** (`vaporspec`) — VaporSpec — build 2026-09-28
- [ ] **Nintendo - Game Boy Advance (VBA Next)** (`vba_next`) — Game Boy Advance — build 2026-09-28
- [ ] **Nintendo - Game Boy Advance (VBA-M)** (`vbam`) — Game Boy/Game Boy Color/Game Boy Advance — build 2026-09-28
- [ ] **Commodore - C128 (VICE x128)** (`vice_x128`) — C128 — build 2026-09-28
- [ ] **Commodore - C64 (VICE x64, fast)** (`vice_x64`) — C64 — build 2026-09-28
- [ ] **Commodore - C64 (VICE x64sc, accurate)** (`vice_x64sc`) — C64 — build 2026-09-28
- [ ] **Commodore - CBM-II 6x0/7x0 (VICE xcbm2)** (`vice_xcbm2`) — CBM-II — build 2025-02-23
- [ ] **Commodore - CBM-II 5x0 (VICE xcbm5x0)** (`vice_xcbm5x0`) — CBM-5x0 — build 2025-02-23
- [ ] **Commodore - PET (VICE xpet)** (`vice_xpet`) — PET — build 2026-09-28
- [ ] **Commodore - PLUS/4 (VICE xplus4)** (`vice_xplus4`) — PLUS/4 — build 2026-09-28
- [ ] **Commodore - C64 SuperCPU (VICE xscpu64)** (`vice_xscpu64`) — C64 SuperCPU — build 2026-09-28
- [ ] **Commodore - VIC-20 (VICE xvic)** (`vice_xvic`) — VIC-20 — build 2026-09-28
- [ ] **Atari - Jaguar (Virtual Jaguar)** (`virtualjaguar`) — Jaguar — build 2026-09-28
- [ ] **VirtualXT** (`virtualxt`) — PC/XT — build 2026-09-28
- [ ] **Wenquxing (WQXEmu)** (`wqxemu`) — Wenquxing — build 2026-09-28
- [ ] **Sega - Saturn (YabaSanshiro)** (`yabasanshiro`) — Saturn — build 2026-09-28
- [ ] **Sega - Saturn (Yabause)** (`yabause`) — Saturn — build 2026-09-28
- [ ] **Sega - Saturn (Emir)** (`ymir`) — Saturn — build 2026-09-28

### Computer (2)

- [ ] **Elektronika - BK-0010/BK-0011(M)** (`bk`) — BK-0010/BK-0011(M) — build 2026-09-28
- [ ] **Sharp X1 (X Millennium)** (`x1`) — Sharp X1 — build 2026-09-28

### Game engine (13)

- [ ] **Doom 3 (boom3)** (`boom3`) — Doom 3 Game Engine — build 2026-09-28
- [ ] **Cannonball** (`cannonball`) — Outrun Game Engine — build 2026-09-28
- [ ] **ChaiLove** (`chailove`) — ChaiLove — build 2026-09-28
- [ ] **RPG Maker 2000/2003 (EasyRPG)** (`easyrpg`) — RPG Maker 2000/2003 Game Engine — build 2026-09-28
- [ ] **Jump 'n Bump** (`jumpnbump`) — Unspecified — build 2026-09-28
- [ ] **LowRes NX** (`lowresnx`) — LowRes NX — build 2026-09-28
- [ ] **Lua Engine (Lutro)** (`lutro`) — Lutro — build 2026-09-28
- [ ] **Omicron** (`omicron`) — Omicron — build 2026-09-28
- [ ] **Super Bros War** (`superbroswar`) — Unspecified — build 2026-09-28
- [ ] **TIC-80** (`tic80`) — TIC-80 — build 2026-09-28
- [ ] **MicroW8** (`uw8`) — MicroW8 — build 2026-09-28
- [ ] **Vircon32** (`vircon32`) — Vircon32 — build 2026-09-28
- [ ] **WASM-4** (`wasm4`) — WASM-4 — build 2026-09-28

### Game (22)

- [ ] **2048** (`2048`) — 2048 Game Clone — build 2026-09-28
- [ ] **Anarch** (`anarch`) — Anarch — build 2026-09-28
- [ ] **Minecraft (Craft)** (`craft`) — Minecraft Game Clone — build 2026-09-28
- [ ] **Dinothawr** (`dinothawr`) — Dinothawr Game Engine — build 2026-09-28
- [ ] **Cave Story (drs)** (`doukutsu_rs`) — Cave Story Game Engine — build 2026-09-28
- [ ] **Wolfenstein 3D (ECWolf)** (`ecwolf`) — Wolfenstein 3D Game Engine — build 2026-09-28
- [ ] **Gong** (`gong`) — Pong Game Clone — build 2026-09-28
- [ ] **Mr.Boom (Bomberman)** (`mrboom`) — Mr.Boom — build 2026-09-28
- [ ] **Cave Story (NXEngine)** (`nxengine`) — Cave Story Game Engine — build 2026-09-28
- [ ] **Tomb Raider (OpenLara)** (`openlara`) — Classic Tomb Raider engine — build 2026-09-28
- [ ] **Doom (PrBoom)** (`prboom`) — DOOM Game Engine — build 2026-09-28
- [ ] **Flashback (REminiscence)** (`reminiscence`) — Flashback Game Engine — build 2026-09-28
- [ ] **ScummVM** (`scummvm`) — ScummVM Game Engine — build 2026-09-28
- [ ] **The Powder Toy** (`thepowdertoy`) — Physics Toy — build 2026-09-28
- [ ] **Quake (TyrQuake)** (`tyrquake`) — Quake Game Engine — build 2026-09-28
- [ ] **VeMUlator** (`vemulator`) — SEGA Visual Memory Unit — build 2026-09-28
- [ ] **Quake II (vitaQuake 2)** (`vitaquake2`) — Quake II Game Engine — build 2026-09-28
- [ ] **Quake II - Ground Zero (vitaQuake 2 [Rogue])** (`vitaquake2-rogue`) — Quake II Game Engine — build 2026-09-28
- [ ] **Quake II - The Reckoning (vitaQuake 2 [Xatrix])** (`vitaquake2-xatrix`) — Quake II Game Engine — build 2026-09-28
- [ ] **Quake II - Zaero (vitaQuake 2 [Zaero])** (`vitaquake2-zaero`) — Quake II Game Engine — build 2026-09-28
- [ ] **Quake III: Arena (vitaQuake 3)** (`vitaquake3`) — Quake 3 Game Engine — build 2026-09-28
- [ ] **Rick Dangerous (XRick)** (`xrick`) — Rick Dangerous Game Engine — build 2026-09-28

### Utility (2)

- [ ] **Internet Radio (Radio)** (`radio`) — Internet Radio Player — build 2026-09-28
- [ ] **ROM Cleaner** (`romcleaner`) — Unspecified — build 2026-09-28

### Music player (1)

- [ ] **Game Music Emu** (`gme`) — Music — build 2026-09-28

### Media player (1)

- [ ] **Video Processor (V4L2)** (`video_processor`) — Video Processor — build 2026-09-28

### Karaoke player (1)

- [ ] **PocketCDG** (`pocketcdg`) — Music — build 2026-09-28

### Streaming (1)

- [ ] **PSP RemotePlay (RemoteJoy)** (`remotejoy`) — PSP — build 2026-09-28

### Tech demo (1)

- [ ] **Test Core - 3D Engine** (`3dengine`) — 3D Engine — build 2026-09-28

### Unclassified (2)

- [ ] **boom3_xp** (`boom3_xp`) — Not identified in the current info bundle — build 2026-07-04
- [ ] **fbalpha** (`fbalpha`) — Not identified in the current info bundle — build 2022-10-08
