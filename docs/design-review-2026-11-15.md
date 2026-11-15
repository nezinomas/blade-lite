# Design review after the USB suspend finding (2026-11-15)

On 2026-11-14 one cause explained many faults: Windows' USB selective suspend put the Razer USB device to sleep 2 to 5 s after each use (tickets 202, 204, 212; `docs/NOTES.md`). This review asks which guards, retries, timeouts and workarounds in the app were built against symptoms of that sleep, or against other causes nobody measured, and which design choices were wrong from the start.

A review only. No app code changed. The tickets at the end are proposals, not opened.

## How it was done

- Read: `docs/NOTES.md`, `docs/PLAN.md`, `docs/PLAN-done.md`, the USB suspend research (`docs/usb-suspend-effects.md` in the project library), and the code that sends, reads, waits or retries (`crates/razer-device`, `crates/blade-core`, `crates/blade-lite/src/shell*`, `app*`, `key_reader.rs`, `background.rs`, `crates/blade-core/src/poll.rs`).
- Git history for the first commit of each guard (`git log -S`).
- Not done: the app's log was not read (it needs the user's yes; findings 5 and 6 say what it would settle). Nothing was run on the laptop. Laptop check 26 was left alone.

Each finding has: the place, why it was added, the evidence, the risk of keeping it and of removing it, and a recommendation.

## Short answer

1. Most guards are real and stay. They fail safe and send nothing by themselves (finding 9).
2. One number carries a large part of the shell: the 2 s pause after a wake. It is a guess from stage 4 and was never measured (finding 1). Three rules built on it now cost safety or sight: a sleep notice in the pause sends no quit preset (finding 2), a late heat read is thrown away (finding 3), and clicks in the pause do nothing.
3. The key reader was patched nine times around one wrong first choice (finding 4). The real cause of the dead keys was the USB wake report. It can be made much smaller.
4. Two choices hid the root cause for days: the log says each read failure once per 10 minutes with no count (finding 6), and probes did not note the device's power state (finding 10).
5. The app now depends on a registry value that only a log line watches (finding 7).

## Findings

### 1. The 2 s pause after a wake is a guess that many rules rest on

- **Place.** `crates/blade-core/src/apply.rs:16` (`RESUME_PAUSE`). Used by `crates/blade-lite/src/shell.rs:356` (`resuming`), `shell.rs:582` (`begin_pause`), `shell.rs:622` (`wait_out_the_pause`), `shell/ticks.rs:663` (`wake`), `shell/ticks.rs:211` (`follow_reopen`), `shell/razer.rs:68` (`start_after_updater`), `main.rs:162`.
- **Why added.** Stage 4, commit `688e41a` (2026-11-02). Its comment says "A guess". Tickets 98, 107, 137, 162, 169, 171 and 179 then each added a rule on top: no click, no read, no power event, no quit and no sleep send while a pause runs, and only the last pause sends.
- **Evidence it was never checked.** No note records a send that failed right after a Windows wake. The sends around a sleep took 30 to 37 ms (laptop checks 33 and 41). Ticket 191 already sends the quit preset with no pause after a thaw and says "at worst it fails again". The one bad reply recorded near a "wake" came at a USB wake with no Windows sleep (ticket 202, `reply not understood`). After a Windows wake the Razer device was in USB sleep too, so the belief "the controller needs a moment" may itself be a USB suspend symptom. Not proven either way.
- **Risk of keeping.** For 2 s after each wake, open again and updater end: no fan guard read, clicks and the power-mode key do nothing with no word (`shell.rs:769`), and finding 2. The pause state is six fields (`resuming`, `pauses`, `pause_began`, `wake_due`, `redo_at_wake`, `asleep`) and gave review bugs in tickets 162, 179, 191 and 199.
- **Risk of removing.** Not known. After a hibernate the device is set up again, and a send into that could fail or be cut partway. A cut Custom leaves the fans at the target already set (ticket 144).
- **Recommendation.** Do not remove it. Measure it first: proposed ticket 219 and check 52. Then pick: keep 2 s, shorten it, or wait for the first good read (a read sends no setting).

### 2. A sleep notice inside a pause sends no quit preset

- **Place.** `crates/blade-lite/src/shell/ticks.rs:771`.
- **Why added.** Ticket 169 kept the rule "nothing is sent in a pause, the controller may not be ready" (finding 1).
- **Evidence it is the wrong side.** The quit preset is the safe state: fans on auto, then Balanced. Ticket 191 sends the same preset at a second notice with no pause. A handle held after an open again refuses the send by itself (`razer-device/src/reopen.rs:65`). So the pause rule protects nothing here that another guard does not already protect.
- **Risk of keeping.** A sleep within 2 s of a wake (a lid opened and shut again, a wake by mistake) leaves a manual fan or Performance running for the whole Modern Standby sleep. Ticket 169 was built to stop exactly that. It is logged, and nobody reads a log in a sleep.
- **Risk of changing.** A send to a controller that is not ready fails and is logged, which is today's end state. If it is cut partway, the fans-on-auto step goes first, so a part send is no worse than no send.
- **Recommendation.** Send the quit preset at a sleep notice also in a wake's pause. Keep it held while a Razer updater runs. Proposed ticket 220, both reviewers. The user decides, as it changes when a command is sent.

### 3. A late heat read is thrown away

- **Place.** `crates/blade-core/src/poll.rs:156` to `166` (`late`, `slept`, `skipped`).
- **Why added.** Commit `fe50ce2` (2026-11-10, ticket 75 review): a read after a sleep might reach a controller that is not awake. The app did not follow sleep notices then.
- **Evidence it is now mostly unneeded.** Since ticket 169 nothing is read from the sleep notice to the end of the wake's pause. Ticket 171 had to add `read_heat_now` (`poll.rs:171`) to undo this skip after a wake. The `read failed: CPU temperature` lines it was meant to avoid also came at USB wakes with no Windows sleep (ticket 202).
- **Risk of keeping.** After the event loop stood still for more than 10 s (the case tickets 143 and 208 worry about), the fan guard's next reading is skipped and the one after waits 10 s, also in the 2 s guard mode. The guard is blind longest right after a stall, when a fresh reading matters most.
- **Risk of removing.** If Windows sends no sleep notice (power events refused), the first read after a wake goes at once. If it fails with a HID error, the handle is dropped and everything is sent again (finding 5).
- **Recommendation.** After ticket 219 shows whether a read right after a wake works: remove the skip, or keep it only when no power events come. Proposed ticket 223, both reviewers.

### 4. The key reader: nine patches around one wrong first choice

- **Place.** `crates/razer-device/src/hid.rs:254` (`KeySources`, "the first collection to send is the only one read"), `hid.rs:199` (`pick_key_collections`), `hid.rs:393` (`model_devices`), `crates/blade-lite/src/key_reader.rs:28` (`KEY_QUIET_SPELL`), `key_reader.rs:36` (`UNHEARD_SPELLS_BEFORE_ALL`), `key_reader.rs:183` (`keeps_key_state`), `crates/blade-lite/src/background.rs:150` to `235`.
- **History.** Ticket 34: read every collection with the key's usage, the first to send wins, no check of the report ID. Commit `468ae53` (2026-11-07): open again after each quiet spell, because "a handle that went stale over a sleep may never fail" (a guess). Ticket 131: that reopen left the key dead 5 s. Ticket 158: the reopen lost the first press. Ticket 174: an unknown key state lost the first press (laptop check 16 had failed again after 131 and after 158). Ticket 188: keep the sender. Ticket 203: only a key report picks the collection. Ticket 209: the profile names the collection. Ticket 210: log dropped reports. Ticket 215: read all collections once an hour.
- **Evidence of a misread cause.** The dead keys of ticket 202 came from the `05` report that collection Col05 sends at each USB wake: it won "first to send". The fix was the report ID check (203), which the first design lacked. A stale handle that stays silent was never seen. Ticket 132's own text says sleep and hibernate kept the handle, and a HID handle whose device went away fails its read. Check 16 failed three times in a row and got three different fixes. Each fixed a real fault found by reading the code, but nobody looked for one shared cause.
- **Risk of keeping.** It works today. The cost is size and churn: about 720 reopens a day; each reopen opens all 11 Razer HID collections to read their usages (`hid.rs:402`) and cancels a read; `keeps_key_state` (a half-spell rule) and ticket 174's known false step exist only because of the reopen. More rules mean more review bugs.
- **Risk of simplifying.** If a silent stale handle is real after all, the keys die until a restart. A reopen at each Windows wake notice and an hourly reopen cover that. The keys are not a safety part: a step goes through every guard.
- **Recommendation.** One ticket (proposed 224, both reviewers, nothing sent changes): open the profile's collection; open again on a read error, at each wake notice and once an hour; drop the quiet-spell count, the kept sender and the turn reading of many collections. Keep "read all" only as the fallback when the profile's collection is not listed. Low priority.

### 5. One HID error sends every setting again

- **Place.** `crates/razer-device/src/reopen.rs:76` (`keep_if_alive`), `crates/blade-lite/src/shell/ticks.rs:211` (`follow_reopen`).
- **Why added.** Ticket 132, found by reading the code (bug hunt 2026-11-11). Its text says "not seen on the laptop". A dead handle was only ever made on purpose (laptop checks 17, 20, 31).
- **Evidence.** Open question. With USB suspend on, one failed feature call at a wake would drop the handle, open a new one, pause 2 s and run `App::start`: every mode write (stored), boost, fan target, logo and key frame again. The day's count of stored writes in ticket 147 does not include such resends. Whether it ever happened is in the log: `the controller was opened again` lines outside the three checks.
- **Risk of keeping.** A single passing error costs a 2 s blind pause and a full resend. The guard itself is real (a driver install, a device restart), so it stays.
- **Risk of removing.** A dead handle would stop every send until a restart. Do not remove.
- **Recommendation.** Keep. Read the log first. Only if reopens show outside the checks, open a ticket to make one error less costly. Two comments are stale since ticket 194 and should be fixed with the next change there: `reopen.rs:30` and `shell.rs:96` say "each open lists every HID device"; an open now lists paths with no open and opens only the Razer ones.

### 6. The log hides how often a read fails

- **Place.** `crates/blade-lite/src/report.rs:133` (`SaidLately`, 10 minutes, no count), `crates/blade-lite/src/app/events.rs:214` (a failed charger read on a poll is dropped with no line), `crates/blade-lite/src/app.rs:523` and `598` (failed charge limit and logo reads dropped).
- **Why added.** Ticket 50: the log filled with the same line.
- **Evidence it hid the cause.** Laptop check 14 counts "6 `read failed: CPU temperature` lines since 2026-11-11". That is 6 or 6000 failures; the log can't tell. A fault at each 10 s USB wake has a rate, and the rate was the clue. Ticket 202 started from what the user saw (a blink each 10 s), not from the log.
- **Risk of keeping.** The next fault with a rhythm is hidden the same way. Check 26 counts log lines with `failed`, so it undercounts too.
- **Risk of changing.** None for the laptop: log only.
- **Recommendation.** Keep the quiet time. When a kind of error is said again, add how many times it failed since the last line. Log a failed charger read on a poll through the same path. Proposed ticket 221. Rust code in `crates/blade-lite`: both reviewers.

### 7. USB suspend is watched only by a log line

- **Place.** `crates/blade-lite/src/usb_suspend.rs:23` (`report_usb_suspend`) and `107`.
- **Why so.** Ticket 213, user pick A: log only.
- **Evidence it is not enough.** The app is now correct only while `SelectiveSuspendEnabled` stays 0. A device removed in Device Manager, a Windows reset, Razer software or a firmware update may put 1 back (`NOTES.md`, ticket 204; the research doc, part 6). Then the lights drop again, mode `06` may come on battery (ticket 211) and the first reply after each wake may be bad. Nobody reads a log line at start.
- **Risk of keeping.** The old faults return with no word on screen.
- **Risk of changing.** None for the laptop: a registry read and a line of text.
- **Recommendation.** Show it in the window, as the Razer software warning is shown, with the script's name. Let the installer (tickets 39, 148) run or offer `tools\Set-RazerUsbSuspend.ps1`. Proposed ticket 222, both reviewers (Rust in `crates/blade-lite`). The battery-sleep drain with suspend off is still open in check 48; if it is bad, the research doc's option 2 is the next thing to try.

### 8. Fan targets sent again after 2 s (ticket 173): likely real, measured with suspend on

- **Place.** `crates/blade-lite/src/shell/ticks.rs:25` and `155`, `crates/blade-lite/src/app.rs:355`.
- **Evidence it is real.** The stall near 2900 RPM also came with the app's window open, whose 1 s reads keep the USB device awake (ticket 204). So it is likely firmware behaviour, not USB sleep.
- **Evidence it is not certain.** All four `cargo hw` probes ran with suspend on and read each 5 s, so the device slept between reads.
- **Recommendation.** Keep. One probe run with suspend off settles it (proposed check 53, after check 26 ends). If no stall comes, the resend can go.

### 9. Guards that stay as they are

These are not USB suspend symptoms, they fail safe, and they send nothing on their own:

- The 1 ms pause between send and read (`razer-device/src/lib.rs:60`): measured back to back, with the device awake.
- A wrong transaction ID is read once more, Busy is read again, nothing is ever sent twice (`lib.rs:178`). Busy never came on the laptop (check 9), but these paths only read.
- The 3 resends of a failed preset (ticket 88), so Performance does not stay on battery.
- A weak charger at once, the full charger only on the second read (ticket 136).
- A CPU reading of 0 °C counts as not read (`razer-proto/src/readings.rs:70`). The 0 °C replies seen on battery (ticket 86) were likely USB wake replies; the guard is right either way.
- The fan guard and its GPU rules, the 3 s feature limits (197, 200), the watchdog, the one-copy lock, the login retries, the quit, session-end and sleep quit preset, the preset read-back (152), the hold while Razer software runs.

### 10. Design choices from the start

- **Every controller call runs on the window's thread.** Tickets 143, 197, 200, 201, 206, 208, 214 and 216 all patch "the event loop was held". A controller thread with a queue would end that class of fault. It changes when things are sent, so it is a large and risky change. Not now; look again if tickets 208 and 216 do not end the stalls.
- **The lights live only in memory of the controller.** Key frames go with no storage byte, and the app paints again at each resume. That works only while the device never USB-sleeps, which is why finding 7 matters. A stored static colour would survive, but the red keys need frames and flash wear is not known. Keep.
- **Probes did not note the device's power state.** The probe of tickets 186 and 187 saw the Razer device "awake the whole time" because the window was open (corrected in ticket 204). Rule for `NOTES.md` (proposed, with ticket 219): each probe and `cargo hw` run prints the Razer device's power state and the four suspend values first, and a note says which sleep it means: Windows sleep, hibernate, USB sleep or GPU sleep.
- **Same symptom, three fixes.** Check 16 (finding 4). The retro rule "sooner if the same thing goes wrong twice" exists; it was not applied to laptop checks that fail twice.

### 11. Facts in `NOTES.md` measured with USB suspend on

Dated before 2026-11-14, so the device slept between reads. Mark them, and measure again when cheap:

| Fact | Why it may be a suspend effect |
| --- | --- |
| Some reads on battery gave 0: CPU temperature, a fan (ticket 86) | First reply after a USB wake |
| The laptop felt hot with still fans on battery after a kill (ticket 86) | With no app the device slept; mode `06` came in that state (ticket 204) |
| The lid logo comes back on after a sleep (ticket 44) | The logo is sent with storage 0, like the key frames that fell back at each USB wake |
| The manual fan stall near 2900 RPM (ticket 173) | Finding 8 |
| Mode `06` on battery (ticket 211) | "Likely, not proven"; needs `-Undo` to prove |

## Proposed tickets (not opened)

In the order I would do them.

- **220** Send the quit preset at a sleep notice also in a wake's pause (finding 2). Both reviewers. The user decides.
- **221** Count throttled read failures in the log, and log failed charger reads on polls (finding 6). Both reviewers.
- **222** Show "USB suspend is on" in the window; the installer runs or offers the script (finding 7). Both reviewers.
- **219** Wake probe: a read-only `cargo hw` test (app quit) that waits for Windows' wake notice, then reads the firmware version at once and each 100 ms for 3 s, and prints each time and result, with the device's power state and suspend values first (findings 1, 3, 10). Both reviewers. Laptop check 52.
- **223** After 219: remove or narrow the late heat read skip, and set the wake pause from what was measured (findings 1, 3). Both reviewers.
- **224** Make the key reader small (finding 4). Both reviewers. Low priority.

## Proposed laptop checks (not added to the plan)

- **52** Wake probe (219). The user quits the app, starts the probe, sleeps from the Start menu and wakes, then closes the lid (hibernate) and opens it. Claude reads the output.
- **53** Fan stall with suspend off (finding 8). Claude runs `custom_from_balanced_and_again_and_what_the_fans_do` with the user's yes, app quit, after check 26 ends.
- **54** Log count (findings 5, 6). Claude reads `%APPDATA%\blade-lite\blade-lite.log` (a file read only, with the user's yes): error lines per day and per kind before and after 2026-11-14, and each `the controller was opened again` line.

Blocked: the log read (it waits for the user's yes).
