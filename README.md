# Knave Settings

Native desktop settings for Knave, built with `knave-app` and `knave-ui`.

Run from this checkout on Wayland or X11:

```sh
cargo run --locked
```

Install the release executable, desktop launcher, and icon:

```sh
scripts/install.sh --user
scripts/install.sh --system
```

User installation honors `XDG_DATA_HOME` and `XDG_BIN_HOME`, defaulting to
`~/.local/share` and `~/.local/bin`. System installation defaults to
`/usr/local`, honors `PREFIX`, and requests sudo when needed. The launcher
looks for `knave-settings` on your PATH.

**Save & reload** writes the Knave configuration and runs `knave reload`.
Leave **Custom theme name** blank to use the built-in Knave theme.

In **Display**, choose **Import wallpaper…** to browse for a PNG or JPEG.
Settings shows a preview with the selected fit and stores the image as PNG
when saved. **Use theme default** clears the custom wallpaper.

In **Startup & dock**, choose installed apps to open at login or pin in
Overview. **Window rules** lets you choose an app or match a window by title
or app ID.
