# Known Bugs and Workarounds

## Windows: full-screen flash on the first mouse movement

### Symptom

After the capture hotkey opens the overlay, the first mouse movement can make
the entire screen flash once. Later pointer updates render normally.

### Conditions

- Windows with DWM enabled.
- An NVIDIA driver affected by independent-flip/direct-scanout promotion.
- A topmost, borderless OpenGL/WGL window whose client size exactly matches the
  captured desktop bounds.
- The first pointer update causes another `SwapBuffers` after the window is
  shown and activated.

### Cause

This is not a normal 60 Hz DWM-window repaint and is not caused by the
cross-platform renderer itself. Windows and the display driver can promote the
exact-size popup to a pseudo-fullscreen presentation path. The next buffer swap
then changes presentation mode, which is visible as a full-screen flash.

### Workaround

On Windows, create the native overlay one physical pixel wider than the
captured desktop. The extra column remains outside the visible desktop. Clamp
`WM_SIZE` back to the captured width and height so rendering, hit testing,
selection coordinates, and exports continue to use the original logical
canvas.

The invariant is implemented by `FULLSCREEN_ESCAPE_MARGIN` in
`src/platform/windows/overlay.rs`. Do not remove the extra native column or resize
the logical canvas to include it without retesting on affected NVIDIA systems.

The workaround intentionally keeps this short-lived capture overlay on the DWM
composition path. Its direct performance cost is negligible; it can forgo a
driver direct-scanout optimization, but stable composition is required for this
overlay and is preferable to the mode-transition flash.

### Rejected experiments

The following measures did not address the driver promotion heuristic and
should not be reintroduced as flicker fixes:

- Rendering another full-screen frame immediately after activation.
- Calling `DwmFlush` repeatedly during overlay startup.
- Reading front/back framebuffer pixels around the first mouse frames.
- Logging and timing every paint, activation, render, and pointer message.

The hidden first render remains intentional: it prevents a newly created WGL
window from briefly exposing its background before a complete frame exists.
The close-time `DwmFlush` also remains intentional: it lets DWM stop using the
dimmed front buffer before the OpenGL resources are destroyed.

### Regression check

On an affected Windows/NVIDIA machine:

1. Trigger capture repeatedly from a normal desktop window.
2. Move the mouse immediately after the overlay appears.
3. Verify there is no full-screen flash and pointer highlighting follows the
   live cursor.
4. Cancel and confirm captures, then verify no dimmed frame remains on screen.
5. Repeat with mixed-DPI or multi-monitor desktop bounds when available.

### Related reports

- [GLFW #527](https://github.com/glfw/glfw/issues/527)
- [GLFW #939](https://github.com/glfw/glfw/issues/939#issuecomment-276084929)
- [SDL #12791](https://github.com/libsdl-org/SDL/issues/12791)
- [winit #4116](https://github.com/rust-windowing/winit/issues/4116)
- [Slint #10031](https://github.com/slint-ui/slint/issues/10031)
