# App icon

`stagecraft.svg` is the source: a profile spot on a pipe throwing a beam onto a pool of
light with a spike mark, in the Craft suite's style (flat rounded square, cream object with
a heavy black outline, orange and black accent dots, a fine grid) on theatre red `#c8363f`.

The PNGs are rendered from it (`rsvg-convert -w 1024 -h 1024 stagecraft.svg -o
stagecraft-1024.png`, likewise 256): `scripts/macos-dmg.sh` makes the `.icns` from the
1024 one, the app embeds the 256 one as its window and Dock icon.
