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

# Tray check (stage 28)

Start the app as above. A blue bolt icon shows in the tray. It may hide under the tray's ^ arrow. Drag it next to the clock to keep it there.

1. **Close the window.** The app keeps running. The icon stays in the tray.
2. **Open the flyout.** Left-click the icon. A small panel with round corners and Mica opens just above the icon.
3. **Close the flyout.** Click the icon again, or click anywhere else. The panel closes.
4. **Flyout controls.** Click Silent. Click your old rate, or another rate and then your old one. Drag Keyboard light, then put it back. Flip Touchpad twice.
5. **Reset to Balanced.** With Silent set, click it. The preset goes back to Balanced, and the button greys out.
6. **Open window.** Click it. The settings window opens and shows the same state.
7. **Menu.** Right-click the icon. "Open window" opens the window. "Quit" ends the app, and the icon goes away.

# Battery presets check (stage 36)

Start the app on the charger, with Performance set.

1. **Unplug.** The header and the buttons say Balanced. Performance and Custom grey out, in the window and the flyout. Clicking them does nothing.
2. **Pick Silent.** It works on battery.
3. **Plug in.** The preset you picked last comes back. If you picked nothing on battery, Performance comes back.

# One copy and autostart check (stage 29)

Start with the app closed.

1. **Second copy.** Start the app, close its window, then start it again. No second icon appears. The first copy opens its window.
2. **Window behind others.** Open the window, then minimise it, or put another window over it. Start the app again. The window comes to the front.
3. **Start with Windows.** Right-click the icon. Tick "Start with Windows".
4. **Sign out and in.** The icon appears in the tray. No window opens. The saved preset and lighting are back.
5. **Task Manager.** In Task Manager, Startup apps, disable Blade Lite. Quit the app and start it again. "Start with Windows" is not ticked. Tick it. Task Manager shows it enabled again.
6. **Turn it off.** Untick "Start with Windows". Sign out and in. The app does not start.
