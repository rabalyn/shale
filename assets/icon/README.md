# App icon

`shale.svg` is the source of the Shale app icon: three cleaved slate plates stacked on a dark
blue-grey rounded square, each with a light edge where the layers split. The PNG files
(`shale-16.png` … `shale-512.png`) are renders of it, made with:

```sh
for n in 16 32 48 64 128 256 512; do rsvg-convert -w $n -h $n shale.svg -o shale-$n.png; done
```

Using the icon for the window and the release packages is raba/shale#213.
