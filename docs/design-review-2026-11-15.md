# Design review after the USB suspend finding (2026-11-15)

On 2026-11-14 one cause explained many faults: Windows' USB selective suspend put the Razer USB device to sleep 2 to 5 s after each use (tickets 202, 204, 212; `docs/NOTES.md`). This review asks which guards, retries, timeouts and workarounds in the app were built against symptoms of that sleep, or against causes nobody measured, and which design choices were wrong from the start.

A review only. No app code changed. The user picked four tickets to open (2026-11-15): 219 to 222 in `docs/PLAN.md`. The rest are proposals, not opened.

## How it was done

- Read: `docs/NOTES.md`, `docs/PLAN.md`, `docs/PLAN-done.md`, the USB suspend research (`usb-suspend-effects.md`, in the project library, not in this repo), and the code that sends, reads, waits or retries (`crates/razer-device`, `crates/blade-core`, `crates/blade-lite/src/shell*`, `app*`, `key_reader.rs`, `background.rs`, `crates/blade-core/src/poll.rs`).
- Git history for the first commit of each guard (`git log -S`).
- The app's log, `%APPDATA%\blade-lite\blade-lite.log` (a file read only, with the user's yes): 438 lines, 2026-11-07 to 2026-11-15 17:18Z.
- The `informed-reviewer` checked the first draft. It found the risk of two proposals wrong or understated and three claims too strong; this version has its fixes.
- Not done: nothing was run on the laptop. Laptop check 26 was left alone.

Each finding has: the place, why it was added, the evidence, the risk of keeping it and of removing it, and a recommendation.

## Short answer

1. The code was not wrong from the beginning in a way that risks the laptop. Almost every guard is real, fails safe, and stays (finding 9). The log shows most of them never fired (below).
2. No guard was found that is harmful now that suspend is off.
3. One number carries a large part of the shell and was never measured: the 2 s pause after a wake (finding 1). The right step is to measure it, not to remove it.
4. Two parts are bigger than they need to be: the late heat read skip (finding 3) and the key reader (finding 4). Both work today.
5. Two weak spots made the search slow: probes did not note the device's power state (finding 10), and the app depends on a registry value that only a log line watches (finding 7).

## What the log says

- **Wakes.** 11 wake lines. No send or read error follows any of them. So with the 2 s pause the wake's sends always worked. The log can't say whether they would work with no pause.
- **Opens again.** `the controller was opened again` came once, at 2026-11-11 15:01:47Z, in laptop check 17 (the device restarted on purpose). The only `the HID layer failed` lines are in the same 5 s. So the reopen never fired on its own.
- **Bad replies.** `read failed: CPU temperature: reply not understood` came 3 times: 2026-11-10 14:29Z and 14:35Z, and 2026-11-14 08:26Z. All before USB suspend was turned off (2026-11-14 evening); none in the day since. A kind of error is said at most once per 10 minutes, so 3 lines mean few failures, not a failure at each wake: a fault at each 10 s wake would give about 144 lines a day. So bad replies at a USB wake were rare.
- **Never in the log.** No Busy line, no reply for another request, no feature time-out, no failed preset send, no `charger` line of any kind. So the Busy read-again, the second read on a wrong transaction ID, the 3 s limits, the 3 resends and the charger's fail-safe have not fired in the app on this laptop.
- **Keys.** `the power-mode key is read on ...Col05` came 3 times on 2026-11-14 morning (07:17Z to 08:31Z), then only Col04 after ticket 203: the wake report took the key reader, as ticket 202 found. On 2026-11-11 15:01:42Z (check 17) the key read failed with `The device is not connected`: a device that goes away does fail the key read on this laptop.

## Findings

### 1. The 2 s pause after a wake is a guess that many rules rest on

- **Place.** `crates/blade-core/src/apply.rs:16` (`RESUME_PAUSE`). Used by `crates/blade-lite/src/shell.rs:356` (`resuming`), `shell.rs:582` (`begin_pause`), `shell.rs:622` (`wait_out_the_pause`), `shell/ticks.rs:663` (`wake`), `shell/ticks.rs:211` (`follow_reopen`), `shell/razer.rs:68` (`start_after_updater`), `main.rs:162`.
- **Why added.** Stage 13 (apply and re-apply), commit `688e41a` (2026-11-02). Its comment says "A guess". Tickets 98, 107, 137, 162, 169, 171 and 179 then each added a rule on top: no click, no read, no power event, no quit and no sleep send while a pause runs, and only the last pause sends.
- **Evidence.** It was never measured. No note and no log line records a send that failed right after a Windows wake, and none records a send tried with no pause. So nothing says the pause is needed, and nothing says it is not. Whether the belief came from USB suspend is not known: bad replies at USB wakes were rare (the log).
- **Risk of keeping.** For 2 s after each wake, open again and updater end: no fan guard read, and clicks and the power-mode key do nothing with no word (`shell.rs:769`). The pause state is six fields (`resuming`, `pauses`, `pause_began`, `wake_due`, `redo_at_wake`, `asleep`) and gave review bugs in tickets 162 and 179.
- **Risk of removing.** Not known. After a hibernate the device is set up again, and a set command into that could fail or be cut partway. A cut Custom leaves the fans at the target already set (ticket 144).
- **Recommendation.** Keep it. Measure first: ticket 219 and check 52. That probe only reads, so it can show when the controller answers after a wake, not that a set command is safe then. A shorter pause for set commands needs the user's pick after 219, and its own check.

### 2. A sleep notice inside a pause sends no quit preset (small; no change before 219)

- **Place.** `crates/blade-lite/src/shell/ticks.rs:771`.
- **Why added.** Ticket 169: nothing is sent in a pause.
- **How big it is.** Smaller than it looks. In a wake's pause the wake's settings have not gone yet, so the controller still runs the quit preset of the sleep before (`ticks.rs:726`). A manual fan runs on in the new sleep only if that earlier quit preset failed or was skipped (two faults in a row), or in the pause after a Razer updater. After an open again the new handle is held and refuses every send anyway.
- **Risk of changing.** Real. The quit preset of a saved manual Custom is two steps: Custom with the fans on auto and the saved boosts, then Balanced (`crates/blade-lite/src/app/events.rs:308`). Sent over a controller that runs Balanced and cut after the first step (a controller not ready, or Windows freezing the app), it would leave Custom with boosts 3 and 3 for the whole sleep, where today Balanced stays. Ticket 191 is no precedent: its second notice is refused in a pause too, and its send with no pause is not checked on the laptop.
- **Recommendation.** No change now. After ticket 219: if at all, send in a wake's pause only when the last sleep's quit preset did not go. A later ticket (not opened), low priority, both reviewers, the user decides.

### 3. A late heat read is thrown away

- **Place.** `crates/blade-core/src/poll.rs:156` to `166` (`late`, `slept`, `skipped`).
- **Why added.** Commit `fe50ce2` (2026-11-10, ticket 75 review): a read after a sleep might reach a controller that is not awake. The app did not follow sleep notices then.
- **Evidence it is now mostly unneeded.** Since ticket 169 nothing is read from the sleep notice to the end of the wake's pause. Ticket 171 had to add `read_heat_now` (`poll.rs:171`) to undo this skip after a wake. So with power events working, the skip acts only after a stalled event loop.
- **Risk of keeping.** After the event loop stood still for more than 10 s (the case tickets 143 and 208 worry about), the fan guard's next reading is skipped and the one after waits 10 s, also in the 2 s guard mode. The guard is blind longest right after a stall.
- **Risk of removing.** The tick behind a heat read is not only a read: it feeds the fan guard, which can send a fan speed (`shell/popup.rs:61`), and it follows the charger and can send the preset again (`shell/ticks.rs:192`). If Windows sends no power events, a wake is not seen (`ticks.rs:528`), and the skip is then the only wait before such a send. (The 1 s poll with a view open has no such skip at all: in that same rare case it already reads, and may send, at the first poll after a wake.)
- **Recommendation.** Keep the skip when no power events come. With power events, remove it, so a stall does not blind the guard. A later ticket (not opened), after 219, both reviewers.

### 4. The key reader is bigger than it needs to be

- **Place.** `crates/razer-device/src/hid.rs:254` (`KeySources`, "the first collection to send is the only one read"), `hid.rs:199` (`pick_key_collections`), `hid.rs:393` (`model_devices`), `crates/blade-lite/src/key_reader.rs:28` (`KEY_QUIET_SPELL`), `key_reader.rs:36` (`UNHEARD_SPELLS_BEFORE_ALL`), `key_reader.rs:183` (`keeps_key_state`), `crates/blade-lite/src/background.rs:150` to `235`.
- **Two first choices.** (a) Ticket 34 and 96: read every collection with the key's usage, the first to send wins, with no check of the report ID. (b) Commit `468ae53` (2026-11-07): open the reports again after each quiet spell, because "a handle that went stale over a sleep may never fail" (a guess; a silent stale handle was never seen).
- **What followed.** From (b): ticket 131 (the reopen left the key dead 5 s) and ticket 158 (the reopen lost the first press). From (a): ticket 188 kept the sender, so the `05` report that Col05 sends at each USB wake took the reader and the keys died (ticket 202); ticket 203 added the report ID check, the real fix; ticket 209 named the collection in the profile; 210 logs dropped reports; 215 reads all collections once an hour.
- **What is not shown.** That the lost first presses of check 16 (tickets 131, 158, 174) had the USB wake as a shared cause. Each has its own cause in the plan, and check 16 passed on 2026-11-12 with suspend still on. Ticket 174 also covers the app start, so it stays whatever the reader does.
- **Risk of keeping.** It works today. The cost is size and churn: about 720 reopens a day; each reopen opens all 11 Razer HID collections to read their usages (`hid.rs:402`) and cancels a read; `keeps_key_state` and the kept sender exist only for the 2 min reopen.
- **Risk of simplifying.** If a silent stale handle is real, the keys die until the next reopen; a reopen at each Windows wake notice and each hour covers that. Ticket 215's hourly read of all collections (check 51 passed) is the way back if firmware moves the keys: keep it. The keys are no safety part: a step goes through every guard (`shell.rs:761`).
- **Recommendation.** Ticket 221 (both reviewers, nothing sent changes). Its `PLAN.md` line is the brief and wins over this text: first a read-only probe of the stale handle guess; then, only if no run shows a silent stale handle, open the profile's collection, open again only after a read error, and drop the 2 min reopen; the rules of tickets 131, 174, 203, 210 and 215 stay. Low priority.

### 5. One HID error sends every setting again (keep)

- **Place.** `crates/razer-device/src/reopen.rs:76` (`keep_if_alive`), `crates/blade-lite/src/shell/ticks.rs:211` (`follow_reopen`).
- **Why added.** Ticket 132, found by reading the code (bug hunt 2026-11-11). A dead handle was only ever made on purpose (laptop checks 17, 20, 31).
- **Evidence.** The worry: with USB suspend on, one failed feature call at a wake would drop the handle, open a new one, pause 2 s and run `App::start`: every mode write (stored), boost, fan target, logo and key frame again. The log says it did not happen: one open again in 9 days, the one of check 17. USB wakes gave a few bad replies, not HID errors, so the handle was kept.
- **Recommendation.** Keep as it is; no ticket. Two comments are stale since ticket 194: `reopen.rs:30` and `shell.rs:96` say "each open lists every HID device"; an open now lists paths with no open and opens only the Razer ones. Fix them with the next change there (Rust files in `crates/`: both reviewers).

### 6. The log has no count for a repeated read failure (small)

- **Place.** `crates/blade-lite/src/report.rs:133` (`SaidLately`, 10 minutes, no count), `crates/blade-lite/src/app/events.rs:214` (a failed charger read on a poll is dropped with no line), `crates/blade-lite/src/app.rs:523` and `598` (failed charge limit and logo reads dropped).
- **Why added.** Ticket 50: the log filled with the same line.
- **Evidence.** A line stands for 1 to about 60 failures in its 10 minutes at the 10 s poll. For the 3 bad replies that was enough to say "rare"; it is not enough to say how rare, and a failed charger read on a poll leaves nothing at all.
- **Risk of changing.** None for the laptop: log only.
- **Recommendation.** Keep the quiet time. When a kind of error is said again, add how many times it failed since its last line. Log a failed charger read on a poll through the same path. A later ticket (not opened), low priority. Rust code in `crates/blade-lite`: both reviewers.

### 7. USB suspend is watched only by a log line

- **Place.** `crates/blade-lite/src/usb_suspend.rs:23` (`report_usb_suspend`) and `107`.
- **Why so.** Ticket 213, user pick A: no change to what is sent (over a repaint after each wake). The app logs at start.
- **Evidence it is thin.** The fixes of 2026-11-14 hold only while `SelectiveSuspendEnabled` stays 0. A new device setup, such as a driver reinstall, may put 1 back (`NOTES.md`, ticket 204); the research doc also names a device removed in Device Manager, a Windows reset and Razer software (reasoned, not seen). Then the lights drop and blink again and mode `06` may come on battery (ticket 211, likely, not proven). Nobody reads a log line at start.
- **Also with suspend on again.** A charger reply not understood on a poll counts as a weak charger at once (`events.rs:216`): Balanced is sent, and the saved preset again after two full reads, each with stored mode writes. The log has no such line so far. It fails safe, so it stays.
- **Risk of changing.** None for the laptop: a registry read and a line of text.
- **Recommendation.** Show it in the window, as the Razer software warning is shown, with the script's name. Let the installer (tickets 39, 148) run or offer `tools\Set-RazerUsbSuspend.ps1`. Ticket 220, both reviewers (Rust in `crates/blade-lite`). The battery drain in a Start-menu sleep with suspend off is still open in check 48.

### 8. Fan targets sent again after 2 s (ticket 173): real, keep

- **Place.** `crates/blade-lite/src/shell/ticks.rs:25` and `155`, `crates/blade-lite/src/app.rs:355`.
- **Evidence it is no suspend effect.** The probe `the_users_custom_with_and_without_the_app_reads` ran one half with the app's reads each second, which keeps the USB device awake (it sleeps only after 2 to 5 s idle), and the fans still held 2900 RPM. The user saw the same with the window open.
- **Recommendation.** Keep the resend. Why the fans stall is still not known; ticket 222 (a) looks for the cause.

### 9. Guards that stay as they are

They fail safe, and they only read or send the whole checked preset anew:

- The 1 ms pause between send and read (`razer-device/src/lib.rs:60`): measured back to back, with the device awake.
- A wrong transaction ID is read once more, Busy is read again, nothing is ever sent twice (`lib.rs:178`). Busy came on the laptop only without the 1 ms pause (`NOTES.md`).
- The 3 resends of a failed preset (ticket 88), so Performance does not stay on battery.
- A weak charger at once, the full charger only on the second read (ticket 136).
- A CPU reading of 0 °C counts as not read (`razer-proto/src/readings.rs:70`).
- The fan guard and its GPU rules, the 3 s feature limits (197, 200), the watchdog, the one-copy lock, the login retries, the quit, session-end and sleep quit preset, the preset read-back (152), the hold while Razer software runs.
- The app's own HID code (tickets 186, 194), the largest work done against a suspend symptom: an open of every HID device woke the sleeping Razer device. It stays right with suspend off: the user's rule is that readers wake no hardware, and it keeps other devices unopened.

### 10. Design choices from the start

- **Every controller call runs on the window's thread.** Tickets 143, 197, 200, 201 and 208 guard against a controller call that holds the event loop. A controller thread with a queue would end that class. It changes when things are sent, so it is a large and risky change. Not now. (Tickets 206, 214 and 216 are a GPU read and a window redraw; a controller thread would not help them.)
- **The key colours live only in the controller's memory.** Key frames go with no storage byte, and the app paints again at each resume. That holds only while the device never USB-sleeps, which is why finding 7 matters. A stored static colour would survive, but the red keys need frames and flash wear is not known. Keep.
- **Probes did not note the device's power state.** The probe of tickets 186 and 187 saw the Razer device "awake the whole time", likely because the window was open (ticket 204). Rule proposed for `NOTES.md`, with ticket 219: each probe and `cargo hw` run prints the Razer device's power state and the four suspend values first, and a note says which sleep it means: Windows sleep, hibernate, USB sleep or GPU sleep.

### 11. Facts in `NOTES.md` measured with USB suspend on

Dated before 2026-11-14, so the device may have slept between reads. Each "maybe" is a guess, not shown:

| Fact | Why it may be a suspend effect |
| --- | --- |
| The CPU temperature read 0 once on battery (ticket 86) | Maybe a first reply after a USB wake |
| The laptop felt hot with still fans on battery after a kill (ticket 86) | With no app the device slept; mode `06` came in that state (ticket 204) |
| The lid logo comes back on after a sleep (ticket 44) | The logo is sent with storage 0, like the key frames that fell back at each USB wake |
| Mode `06` on battery (ticket 211) | "Likely, not proven"; needs `-Undo` to prove |

## Tickets

Opened in `docs/PLAN.md` (user's pick, 2026-11-15; 218 is in use on another thread's branch):

- **219** Wake probe: a read-only `cargo hw` test (app quit) that waits for Windows' wake notice, then reads the firmware version at once and each 100 ms for 3 s, and prints each time and result, with the device's power state and suspend values first (findings 1, 10). Both reviewers. Laptop check 52.
- **220** Show "USB suspend is on" in the window; the installer runs or offers the script (finding 7). Both reviewers.
- **221** The key reader (finding 4): first a read-only probe of a key handle kept over a sleep and a hibernate, then make the reader small with what it shows. Both reviewers. Low priority. Laptop check 53.
- **222** Find the causes behind three workarounds that work but have no proven cause: the fan targets sent again after 2 s (ticket 173, finding 8), the lid logo sent again after each sleep (ticket 44, finding 11), and the 60 Hz tried up to 10 times after a wake (ticket 82; not in this review, from the user's request). Investigation first. One reviewer for notes, both for any code. Laptop check 54.

Proposed, not opened, with no number yet (`PLAN.md` gives one when a ticket is opened). In the order I would do them:

- After 219: remove the late heat read skip while power events come, and decide the wake pause with the user (findings 1, 3). Both reviewers.
- Count throttled read failures in the log, and log failed charger reads on polls (finding 6). Both reviewers. Low priority.
- After 219, if at all: the quit preset at a sleep notice in a wake's pause, only when the last sleep's quit preset did not go (finding 2). Both reviewers. The user decides.

## Laptop checks

- **52** Wake probe (ticket 219), after check 26 ends (the app must be quit). The user quits the app, starts the probe, sleeps from the Start menu and wakes, then closes the lid (hibernate) and opens it. Claude reads the output. It goes into "Still to check on the laptop" when ticket 219 is built.
- **53** Key handle probe (ticket 221): as check 52, and the user presses Fn once after each wake.
- **54** Lid logo probe and the 60 Hz log (ticket 222, parts b and c). The steps are in the ticket's `PLAN.md` line.

Blocked: nothing was blocked.
