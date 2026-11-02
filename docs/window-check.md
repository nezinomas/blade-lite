# Window check (stage 27)

The window has no unit tests. Check it by hand on the laptop.

Start it:

```
cargo run -p blade-lite
```

A console window opens too in debug builds. Ignore it.

Each step says what to put back after.

1. **Look.** Mica shows behind the window. Buttons use the Windows accent colour. The header shows the CPU temperature, both fans, CPU load and Battery.
2. **Live numbers.** The fan speeds and the CPU load change about once a second.
3. **Refresh rate.** Click 120 Hz. The screen switches. Click your old rate to put it back.
4. **60 Hz on battery.** Turn the switch on, then off. The switch follows the click. Put it back as it was.
5. **Native mode.** Click the other mode. The yellow "Restart to apply" bar shows. Click your old mode. The bar goes away. Do not restart.
6. **Performance.** Click Silent. The header says Silent. Click Custom. Three sliders show. Move CPU boost one step. Click your old preset to put it back.
7. **Brightness.** Drag the slider to 50%. The keyboard dims. Drag it to 0%. The keyboard goes dark and the colours grey out.
8. **Colour.** Set brightness back up. Click the red swatch. The keys turn red. Type `#00B7C3` in the Custom box and press Enter. The keys turn teal. Click the white swatch to put it back.
9. **Touchpad.** Flip the switch. The touchpad turns on or off. Flip it back.
10. **Close and open again.** The preset and the lighting you left are still set.

The app now remembers the lighting in `%APPDATA%\blade-lite\config.toml`. The controller can't report it, so the window shows 100% white until you set it once.
