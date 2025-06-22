# PineNote Service

PineNote Service aims to be a central, dbus-aware service to manage various
PineNote specific configurations.

## Usage

Pending documentation.

## Format

### Render Hints
#### Human readable
When represented as a string, the rendering hints uses the following
representation:  
`<BITDEPTH>[|<CONVERT>][|<REDRAW>]`  
`BITDEPTH`:
- Y4 -> 4bpp gray
- Y2 -> 2bpp gray
- Y1 -> 1bpp B/W

`CONVERT`:
- T -> Uses thresholding. Useful with `BITDEPTH` `Y2` or `Y1`,
- D -> Uses dithering. Useful with `BITDEPTH` `Y2` or `Y1`  
Defaults to thresholding.

`REDRAW`:
- R -> Enable fast drawing followed by a redraw in higher quality
- r -> Disable fast drawing.  
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
- AppRegister -  `i -> s` -  Takes a process pid and returns an arbitrary
  application key.
- AppRemove - `s` - Takes an application key, and remove the application and
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
window -> (s(iiii)sbi)
  title -> s
  area -> (iiii)
    x1 -> i
    y1 -> i
    x2 -> i
    y2 -> i
  hint -> s
  visible -> b
  z-index -> i
```

##### Window Management Method
`area` is a rectangle defined by its top-left and bottom-right coordinate.

`hint` is a string, which can either be empty to use default hints, or respect
the [human readable](#human-readable) format

HintMgr1 interface has the following methods to manage Window:
- WindowAdd - `s(s(iiii)sbi) -> s` - Take an application key and a `window`.
  Returns an arbitrary key to refer back to this window.
- WindowRemove - `s -> ()` - Take a window key, and remove the window.
- WindowUpdate - `s(s(iiii)sbi) -> ()` - Take a window key and perform an update
  of all the window field. This method should be used when several fields need
  to be updated, since every fields could trigger an update.
- WindowUpdateArea - `s(iiii) -> ()` - Set the new window area.
- WindowUpdateHint - `ss -> ()` - Set or unset the window rendering hint
- WindowUpdateTitle - `ss -> ()` - Update the window title
- WindowUpdateVisible - `sb -> ()` - Set or Unset the 'visible' flag for the
  window.
- WindowUpdateZindex - `si -> ()` - Set the window z-index.
