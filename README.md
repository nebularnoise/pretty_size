<div id="top"></div>

<br />
<div align="center">
  <h3 align="center">pretty-size</h3>

  <p align="center">
    human-readable .elf size report, inspired by <a href="https://github.com/wntrblm">@wntrblm</a>'s fw_size
  </p>
</div>

<!-- ABOUT THE PROJECT -->

## About The Project

This is a shameless rust re-write of the excellent [fw_size](https://github.com/wntrblm/wintertools/blob/main/wintertools/fw_size.py), written by [@wntrblm](https://github.com/wntrblm) as part of the wintertools python package.

Here's why:

1. I wanted more stuff (I initially contributed to fw_size to get what I wanted)
2. A colleague of mine had issues running fw_size because of Python2 / Python3 being a hot mess on their distro
3. I wanted an excuse to try out the Rust programming language

### Built With

- [colored](https://crates.io/crates/colored)
- [clap](https://crates.io/crates/clap)
- [serde](https://crates.io/crates/serde)
- [ldscript-parser](https://crates.io/crates/ldscript-parser)

### Installation

A quick and dirty way :

```sh
 cargo install --git https://github.com/nebularnoise/pretty_size
```

## Usage

Here's an example, based on my usage.

```sh
pretty_size \
--size-prog ~/dev_tools/arm-none-eabi-size \
--ld STM32H743BITx_FLASH.ld \
-e section_edits.json \
a.elf
```

## Linker script parsing

Here are the memory regions defined in my linker script.

```ld
MEMORY
{
bootloader (rx) : ORIGIN = 0x08000000, LENGTH = 0x20000
FLASH (rx)      : ORIGIN = 0x08020000, LENGTH = 0x1E0000
DTCMRAM (xrw)   : ORIGIN = 0x20000000, LENGTH = 0x20000
DMA_RAM (xrw)   : ORIGIN = 0x24000000, LENGTH = 0x8000
RAM (xrw)       : ORIGIN = 0x24008000, LENGTH = 0x78000
RAM_D2 (xrw)    : ORIGIN = 0x30000000, LENGTH = 0x48000
RAM_D3 (xrw)    : ORIGIN = 0x38000000, LENGTH = 0x10000
ITCMRAM (xrw)   : ORIGIN = 0x00000000, LENGTH = 0x10000
SDRAM (xrw)     : ORIGIN = 0xC0000000, LENGTH = 0x2000000
}
```

This is just to illustrate two things:

- ~~Firstly, I had to put all the origin and sizes in hex, as the `ldscript-parser` crate seemed to not like the `2M`/ `64k`, etc. syntax~~ (EDIT: actually, `ldscript-parser` does not support expressions, so arithmetic in ORIGIN and LENGTH was the culprit.
- Secondly, this example will be referenced in the following section.

## Section edits

Section edits allow to groups regions and ignoring sections. They are to be stored in a json file, and passed to `pretty-size` with the `-e` argument.

Here's an example:

```json
[
  {
    "GroupRegions": {
      "region_to_insert_as_section": "bootloader",
      "output_region": "FLASH",
      "output_section_name": ".bootloader"
    }
  },
  {
    "GroupRegions": {
      "region_to_insert_as_section": "DMA_RAM",
      "output_region": "RAM",
      "output_section_name": ".dma_ram"
    }
  },
  {
    "Ignore": {
      "region_name": "FLASH",
      "section_name_to_ignore": ".padding"
    }
  }
]
```

My linker script used a region called `bootloader` to define a memory location reserved for the bootloader. In the report, I want to see it in the `FLASH` region.

Similarly, I used a region called `DMA_RAM` to easily have a range addresses in RAM, on which I could disable caching, which was causing issues with DMA in my project.

Finally, the `size` program was showing me the `.padding` section I had defined in the linker script, but I don't want to see it in the report.

## Output example

![output.png](https://user-images.githubusercontent.com/33561374/164675856-e2b8f98e-71cb-44ec-9cca-367d1dd2dff3.png)

## License

Distributed under the MIT License. See `LICENSE.txt` for more information.
