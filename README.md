# PineNote Service

PineNote Service aims to be a central, dbus-aware service to manage various
PineNote specific configurations.

## Usage

Pending documentation.

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

The HINT must respect the following format `<BITDEPTH>[|<CONVERT>][|<REDRAW>]`.
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
