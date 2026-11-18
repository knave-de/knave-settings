# Knave Settings

Native desktop settings for Knave, built with `knave-app` and `knave-ui`.

Run from this checkout on Wayland or X11:

```sh
cargo run --locked
```

**Save & reload** writes the Knave configuration and runs `knave reload`.
Leave **Custom theme name** blank to use the built-in Knave theme.

In **Display**, choose **Import wallpaper…** to browse for a PNG or JPEG.
Settings shows a preview with the selected fit and stores the image as PNG
when saved. **Use theme default** clears the custom wallpaper.

In **Startup & dock**, choose installed apps to open at login or pin in
Overview. **Window rules** lets you choose an app or match a window by title
or app ID.
