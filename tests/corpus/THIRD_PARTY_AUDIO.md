# Third-party audio (corpus sources)

Optional real-world fixtures for manifest cases with `requires_source = true`. Files are **not** committed; download with `scripts/fetch_corpus_sources.ps1` (or `.sh`).

Metadata and SHA-256 pins: `sources.toml`.

## Sources

### Martin Amis and creative writing (Interview 1990)

- **Author:** Maximilian Schönherr
- **License:** [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/)
- **Page:** [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Martin_Amis_and_creative_writing_(Interview_1990).mp3)

### Schiphol: geroezemoes in het restaurant

- **Author:** Beeld en Geluid (Netherlands Institute for Sound and Vision), [Geluid van Nederland](https://commons.wikimedia.org/wiki/Commons:Geluid_van_Nederland)
- **License:** [CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/)
- **Page:** [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Schiphol,_geroezemoes_in_het_restaurant_-_SoundCloud_-_Beeld_en_Geluid.ogg)

## Derived test pairs

Manifest cases build A/B pairs by applying a known timing offset (ffmpeg `adelay`) and optional re-encode. Generated pairs are ephemeral (temp dirs) and not redistributed.
