# PineNote Service

PineNote Service aims to be a central, dbus-aware service to manage various
PineNote specific configurations.

## Usage

### Starting the service

The service can be started in a standalone way, either by running the binary
directly, or by starting it through your WM/Compositor config.

#### SystemD support
The [pinenote.service][rsx_sysd] file in [packaging/resources][rsx] contains a
systemd unit definition to manages the service automatically. When enabled, the
unit binds to `graphical-session.target` meaning it should be started with you
WM or Compositor, if it's support the feature.

To use the service, install the file in `/etc/systemd/user/` and reload the user
daemons by running `systemctl --user daemon-reload`. You can then start or
enable the service.

#### DBus Activatable Service
The [org.pinenote.PineNoteCtl.service][rsx_dbus] file in
[packaging/resources][rsx] allows the service to be started via DBus directly.
To achieve this, you first have to install said file to
`/usr/share/dbus-1/services` and the systemd unit in `/etc/systemd/user`

[rsx]: packaging/resources
[rsx_sysd]: packaging/resources/pinenote.service
[rsx_dbus]: packaging/resources/org.pinenote.PineNoteCtl.service

### DBus API
Currently, the only available API to interact with the service is through DBus.

Since there are no clients at the moment, you can use `dbus-send` or `busctl` to
call method, and read or write properties. Specialized client would be able to
register on specific signals to get update on properties.

The service currently uses the well-know name `org.pinenote.PineNoteCtl`, and
the path `/org/pinenote/PineNoteCtl` for every interface it exposes.

Example usage with busctl:
```sh
# Getting & Setting properties
$ busctl --user get-property org.pinenote.PineNoteCtl /org/pinenote/PineNoteCtl org.pinenote.Ebc1 DefaultHintHr 
s "Y4|T|R"
$ busctl --user set-property org.pinenote.PineNoteCtl /org/pinenote/PineNoteCtl org.pinenote.Ebc1 DefaultHintHr s "Y1|D"
$ busctl --user get-property org.pinenote.PineNoteCtl /org/pinenote/PineNoteCtl org.pinenote.Ebc1 DefaultHintHr       
s "Y1|D|r"

# Calling a method
$ busctl --user get-property org.pinenote.PineNoteCtl /org/pinenote/PineNoteCtl org.pinenote.Ebc1 OffScreenOverride 
s "unknown"
busctl --user call org.pinenote.PineNoteCtl /org/pinenote/PineNoteCtl org.pinenote.Ebc1 SetOffScreen s ~/Untitled.png
$ busctl --user get-property org.pinenote.PineNoteCtl /org/pinenote/PineNoteCtl org.pinenote.Ebc1 OffScreenOverride
s "/home/phantomas/Untitled.png"
```

#### org.pinenote.PineNoteCtl

This is the generic 'entry point' interface. 

```sh
$ busctl --user introspect org.pinenote.PineNoteCtl /org/pinenote/PineNoteCtl org.pinenote.PineNoteCtl1
NAME                      TYPE      SIGNATURE RESULT/VALUE FLAGS
.Dump                     method    s         -            -
.ActiveBridge             property  s         "Sway"       emits-change
```

The ActiveBridge property is the only meaningful value on this interface and
show which bridge is active, if any. When no bridges could be started, its value
is 'generic'.

Dump is a debug method, used to dump some informations in the file passed by
parameter.

In the future, this interface will be used for general debugging and some
feature not fitting in other interfaces.

#### org.pinenote.Ebc1

This interface allows low level interaction with the rockchip_ebc kernel driver.

```sh
➜  ~ busctl --user introspect org.pinenote.PineNoteCtl /org/pinenote/PineNoteCtl org.pinenote.Ebc1        
NAME               TYPE      SIGNATURE RESULT/VALUE FLAGS
.CycleDitherMode   method    -         -            -
.CycleDriverMode   method    -         -            -
.DumpFramebuffers  method    s         -            -
.GlobalRefresh     method    -         -            -
.SetOffScreen      method    s         -            -
.DefaultHint       property  (yyb)     2 0 true     emits-change writable
.DefaultHintHr     property  s         "Y4|T|R"     emits-change writable
.DitherMode        property  y         2            emits-change writable
.DriverMode        property  y         0            emits-change writable
.OffScreenDisable  property  b         false        emits-change writable
.OffScreenOverride property  s         "unknown"    emits-change
.RedrawDelay       property  q         100          emits-change writable
```

**Properties**  
*DefaultHint*: Exposes the raw Hint representation, and is meant
for machine interaction.  
*DefaultHintHr*: Exposes the driver default rendering hint, using the
[human readable](#human-readable) format.  
*DitherMode*: Exposes the (dithering algorithm used by the driver.  
*DriverMode*: Exposes the rendering mode used by the driver.  
*OffScreenDisable*: Disables outputting a 'screen saver' image when suspending.  
*OffScreenOverride*: Path to the file that will be shown when suspending.  
*RedrawDelay*: Time to wait before refreshing the pixels when using rendering hints
with the redraw bit set.  

**Methods**  
*CycleDitherMode*: Calling this method selects the next DitherMode available.  
*CycleDriverMode*: Select the next rendering mode.  
*DumpFramebuffers*: Call the debug IOCTL writing its output to a directory.  
*GlobalRefresh*: Triggers a global screen refresh  
*SetOffScreen*: Open an image, and uses it as the picture to display upon
suspend.

#### org.pinenote.HintMgr1

Generic dbus-based compositor bridge. 

```sh
$ busctl --user introspect org.pinenote.PineNoteCtl /org/pinenote/PineNoteCtl org.pinenote.HintMgr1 
NAME                    TYPE      SIGNATURE      RESULT/VALUE FLAGS
.AppRegister            method    i              s            -
.AppRemove              method    s              -            -
.WindowAdd              method    s(s(iiii)sbbi) s            -
.WindowRemove           method    s              -            -
.WindowUpdate           method    s(s(iiii)sbbi) -            -
.WindowUpdateArea       method    s(iiii)        -            -
.WindowUpdateFullscreen method    sb             -            -
.WindowUpdateHint       method    ss             -            -
.WindowUpdateTitle      method    ss             -            -
.WindowUpdateVisible    method    sb             -            -
.WindowUpdateZindex     method    si             -            -
```

More info in the [Bridge Section](#generic-dbus-bridge)

## Format
### Dithering Mode
Select the dithering algorithm/pattern
- 0 -> Bayer
- 1 -> Blue Noise Matrix - 16x16
- 2 -> Blue Noise Matrix - 32x32

### Driver Mode
- 0 -> Normal
- 1 -> Fast
- 8 -> Zero-Waveform

### Render Hints
#### Machine readable
Render Hints are represented as two bytes and a boolean. The first byte
represents the bitdepth, the second one the conversion method, and the boolean
define whether 2 phase rendering should be used.

**BitDepth**:  
*0* -> Y1 - 1bpp B/W
*1* -> Y2 - 2bpp grayscale
*2* -> Y4 - 4bpp grayscake

**Conversion**:  
*0* -> Thresholding
*1* -> Dithering

#### Human readable
When represented as a string, the rendering hints uses the following
representation:  
`<BITDEPTH>[|<CONVERT>][|<REDRAW>]`  

**`BITDEPTH`**:  
*Y4* -> 4bpp grayscale  
*Y2* -> 2bpp grayscale  
*Y1* -> 1bpp B/W  

**`CONVERT`**:  
*T* -> Uses thresholding. Useful with `BITDEPTH` `Y2` or `Y1`  
*D* -> Uses dithering. Useful with `BITDEPTH` `Y2` or `Y1`  
Defaults to thresholding.

**`REDRAW`**:  
*R* -> Enable fast drawing followed by a redraw in higher quality  
*r* -> Disable fast drawing.  
Defaults to being disabled.

## Compositor Bridges

Compositor bridges allow the service to be compositor aware, and to retrieve 
window informations such as their position and render hints, to send them to the
display driver.

Currently only one bridge is supported for SwayWM.

### Sway Bridge

The Sway bridge listen for events on sway-ipc and poll swaytree from time to
time.

When when the tree is recovered, the bridge find all window and floating window.
It then uses the window mark feature to determine the window hint and display
position, and send necessary updates to the service's core.

#### Render Hints
To determine render hints, the bridge uses sway's `mark` feature. Since marks
must be unique, the bridge looks for marks in the following format:
- ebchint:\<UNIQUE>:\<HINT> - These marks are shown in window decoration
- \_ebchint:\<UNIQUE>:\<HINT> - These marks are hidden in window decoration

The `HINT` parameter follows the [human readable](#human-readable) format.

#### Mark Configuration

The easiest way to manage hint marks is to use the `ebcmark.sh`
[script][ebcmark_script]. It uses `swaymsg` and `jq` to find the current mark
if any, and will add/update it on `set` or remove it on `unset`.

[ebcmark_script]: scripts/sway/ebcmark.sh

##### Setting or updating Hint
###### From terminal:
```sh
$ ebcmark.sh set "Y4|R"
# or, for hidden mark
$ ebcmark.sh set "Y4|R" silent
# note: the last parameter value doesn't matter. The script just check if it's
# a non-empty string.
```

###### From sway config:
```
for_window [app_id="mpv"] exec "ebcmark.sh" set "Y1|D"
for_window [app_id="foot"] exec "ebcmark.sh" set "Y2|R"
for_window [app_id="com.github.xournalpp.xournalpp"] exec "ebcmark.sh" set "Y4|R"
```

### Generic DBus Bridge

As an alternative to proper compositor bridges, the dbus object at 
`/org/pinenote/PineNoteCtl` exposes the `org.pinenote.HintMgr1` interface, 
which act as a generic bridge. 

Since this is meant as a replacement for actual Compositor build, some concepts
are used as-is, even if they may not be relevant for your specific use-case.

#### App Management

Applications are used to manage windows, and have a way to remove all window at
once. While the goal is to expand on the feature set (such as setting per
application hints, or retrieving all window for a given application), currently 
only adding/removing applications is supported.

HintMgr1 interface has the following methods:  
*AppRegister* -  `i -> s` -  Takes a process pid and returns an arbitrary
application key.  
*AppRemove* - `s` - Takes an application key, and remove the application and
associated window.  

#### Window Management

A window is primarily defined by an area, a z-index and, optionally, a set of
hints. The z-index defines the 'depth' of the window, with higher z-indices
being closer to the user (z-index is the 'height' in the window stack).

The windows z-indices are used internally to determine which rendering hint will
be sent to the driver, and in which order, to ensure the proper hints are used
to render surfaces.

Hints are optional. If a window doesn't have an associated hint, the default
hint is used instead.

If several window are overlapping at the same z-index, which hint will be used
for the overlapping window is undefined. However, if they z-index is different,
then the hint for the 'higher' window will be used, or the default hint if said
window didn't have a defined hint.

##### DBus Representation

Method using window expect the following signature for the 'window' parameter:
```
window -> (s(iiii)sbbi)
  title -> s
  area -> (iiii)
    x1 -> i
    y1 -> i
    x2 -> i
    y2 -> i
  hint -> s
  visible -> b
  fullscreen -> b
  z-index -> i
```

`area` is a rectangle defined by its top-left and bottom-right coordinate.

`hint` is a string, which can either be empty to use default hints, or respect
the [human readable](#human-readable) format


##### Window Management Method
HintMgr1 interface has the following methods to manage Window:  
*WindowAdd* - `s(s(iiii)sbbi) -> s` - Take an application key and a `window`.
Returns an arbitrary key to refer back to this window.  
*WindowRemove* - `s -> ()` - Take a window key, and remove the window.  
*WindowUpdate* - `s(s(iiii)sbbi) -> ()` - Take a window key and perform an update
of all the window field. This method should be used when several fields need
to be updated, since every fields could trigger an update.  
*WindowUpdateArea* - `s(iiii) -> ()` - Set the new window area.  
*WindowUpdateHint* - `ss -> ()` - Set or unset the window rendering hint  
*WindowUpdateTitle* - `ss -> ()` - Update the window title  
*WindowUpdateVisible* - `sb -> ()` - Set or unset the window 'visible' flag.  
*WindowUpdateFullscreen* - `sb -> ()` - Set or unset the window 'fullscreen'
flag  
*WindowUpdateZindex* - `si -> ()` - Set the window z-index.  
