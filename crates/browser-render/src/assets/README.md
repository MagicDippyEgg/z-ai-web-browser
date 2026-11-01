# Embedded fonts

This crate ships three DejaVu TrueType fonts as embedded assets:

- `serif.ttf` — DejaVu Serif (regular)
- `sans.ttf` — DejaVu Sans (regular)
- `mono.ttf` — DejaVu Sans Mono (regular)

These are used as fallbacks when no suitable system font can be found at
runtime, so the binary works on systems without `fonts-dejavu` installed.

## License

The DejaVu fonts are released under a permissive license that permits
redistribution and embedding. See https://dejavu-fonts.github.io/License.html
for the full text. In summary:

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of the fonts accompanying this license ("Fonts") and associated documentation
> files (the "Font Software"), to deal in the Font Software, including without
> limitation the rights to use, copy, modify, merge, publish, distribute,
> sublicense, and/or sell copies of the Font Software…

The fonts were downloaded from the system package `fonts-dejavu` on Debian 13.
