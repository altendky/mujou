# Project Naming

**Decision: mujou** (無常, impermanence) with domain **mujou.art**.

## Guiding Principles

- Focus on functionality, not output format
- crates.io availability
- Domain availability (.art TLD is the primary option, .garden is secondary)

## Thematic Directions

1. **Functional** -- words describing what the tool does (trace, etch, scribe, contour, vectorize)
2. **Sand / beach / dune** -- the primary output device is a kinetic sand table
3. **Zen / calm / harmony / nature** -- the aesthetic of sand gardens, balance, meditative patterns
4. **Japanese words** -- zen gardens (karesansui) are Japanese; culturally resonant vocabulary

## Word Bank

### Action / Function

trace, etch, scribe, inscribe, walk, stride, hatch, stroke, rake, comb, sweep, furrow, score

### Technical / Pipeline

edge, contour, path, line, vector, raster

### Sand / Beach / Dune

sand, dune, shore, tide, drift, ripple, wave, ebb, grain, oasis

### Zen / Calm / Nature

zen, garden, stone, still, calm, balance, moss, fern, mist, glade

### Japanese

| Word | Kanji | Meaning |
| --- | --- | --- |
| samon / shamon | 砂紋 | Sand pattern (raked sand in zen gardens). Note: 砂 has two on'yomi (sa, sha) so romanization varies |
| nagi | 凪 | Calm, still sea |
| nagisen | 凪線 | Calm + line |
| suji | 筋 | Line, path, sinew |
| tsuyu | 露 | Dewdrop |
| nagisa | 渚 | Water's edge, shore |
| sazanami | さざ波 | Gentle ripples on water |
| karesansui | 枯山水 | Dry landscape zen garden |
| sen | 線 | Line |
| mujou | 無常 | Impermanence |
| mushin | 無心 | No-mind (zen concept) |
| houkime | 箒目 | Rake/broom marks in a zen garden |
| yugen | 幽玄 | Mysterious depth/beauty |

## Decision

**Name:** mujou (無常)
**Domain:** mujou.art
**Crate:** mujou (available on crates.io)

### Final comparison: mujou.art vs. houkime.garden

The final choice came down to these two. mujou.art was chosen for the following reasons:

- **Philosophical resonance over literal description.** Mujou (無常, impermanence) captures
  the defining quality of sand table art — patterns are drawn, admired, then erased to make way
  for the next. "Impermanent art" isn't just a label, it's a statement about what the medium is.
- **The .art TLD does real work.** It directly states what the tool's output is, while mujou
  adds the philosophical dimension. Together they form a complete thought.
- **Compact.** mujou.art is 9 characters before the dot — short, clean, easy to type and remember.
- **Wider recognition.** Mujou/mujo is more widely known in western zen/Buddhist vocabulary than
  houkime, which is a niche gardening term.
- **Room to grow.** A philosophical name gives the brand room to expand beyond sand tables, while
  houkime.garden is more tightly coupled to one specific output form.

houkime.garden was the strongest alternative — "rake marks in a garden" is precisely what you see
in a karesansui, and the .garden TLD made it a harmonious, literal pairing. But for a project name
that people encounter cold, mujou.art invites more curiosity.

### Romanization note

The authentic wāpuro (keyboard) romanization is **mujou** (むじょう), not "mujo." The trailing う
in 常 (じょう, jou) is preserved. This also had a practical benefit: mujo.art was premium-priced
($159.90) while mujou.art is standard ($3.98/yr).

### Cross-language connotations

無常 is a shared CJK character compound, so it carries meaning in Japanese, Chinese, Korean, and
Vietnamese. The readings differ across languages, and so do the cultural associations.

**Japanese (intended meaning).** 無常 (mujō) means impermanence -- the Buddhist teaching that all
conditioned things are transient. It carries a tone of poetic melancholy, adjacent to mono no aware
(物の哀れ). The most famous usage is the opening of the *Tale of the Heike*: 諸行無常 (shogyō
mujō), "all things are impermanent." This is the meaning we intend.

**Chinese (primary concern).** 無常 (wúcháng) does mean impermanence in Buddhist philosophy, but in
common Chinese usage it has a much stronger association with **death and the underworld**.
Specifically:

- **黑白無常** (Hēibái Wúcháng, the Black and White Guards of Impermanence) are two ghost/deity
  figures in Chinese folk religion who escort souls of the dead to the underworld. They are iconic
  and widely recognized in popular culture.
- The phrase 人生無常 (rénshēng wúcháng, "life is impermanent") is commonly used when someone dies
  unexpectedly -- it carries a grim connotation rather than a serene one.
- 無常 can function as a euphemism for death itself ("to meet wúcháng" = to die).

For Chinese-speaking users, 無常 rendered in characters could evoke death, ghosts, and the
underworld rather than the serene zen-garden impermanence we intend.

**Korean.** 무상 (musang), same characters 無常. Carries the Buddhist philosophical meaning
(impermanence) with less of the death-deity association than Chinese, though still somber.

**Vietnamese.** Vô thường, same characters. Similar range of meaning to Korean -- Buddhist
impermanence, without the strong underworld connotation.

**Western languages.** The romanized sound "mujou" does not collide with words in French, Spanish,
Portuguese, Italian, German, Dutch, or other major Western languages. No phonetic concerns.

#### Assessment

| Concern | Severity | Notes |
| --- | --- | --- |
| Chinese death/underworld association | Moderate | 無常 in characters strongly evokes the underworld escorts and death in Chinese folk culture. Risk is lower when encountered as romanized "mujou" in Latin script, higher if bare 無常 is displayed prominently in branding. |
| General somber tone across CJK | Low | Even in Japanese, 無常 is melancholic -- about loss and transience. Arguably a feature given the intent (patterns drawn, admired, erased), but worth noting it is not a cheerful word in any CJK language. |
| Western phonetic collisions | Negligible | "Mujou" has no unfortunate overlap with words in major Western languages. |

#### Existing mitigations

Several aspects of the project already steer toward the intended reading:

- **Japanese romaji.** "Mujou" is unmistakably Japanese romanization. The Chinese reading would be
  "wúcháng." A Chinese speaker seeing "mujou" in Latin script would recognize it as Japanese, which
  creates distance from the Chinese folk-religion meaning.
- **The .art TLD.** "mujou.art" reads as "impermanence + art," steering toward the
  aesthetic/philosophical register. The death/underworld reading does not pair naturally with "art."
- **Subject matter.** Sand patterns, zen gardens, traced paths -- the visual and conceptual context
  of the project naturally aligns with the Buddhist/zen reading, not the underworld one.

#### Available disambiguators

If further clarification is ever needed:

- **諸行無常 (shogyō mujō).** The four-character Buddhist phrase "all things are impermanent" is
  unambiguous across all CJK traditions. Even in Chinese, 諸行無常 is firmly philosophical, not
  morbid. When displaying kanji or providing the "full form" of the name's meaning, prefer this
  phrase over bare 無常.
- **English tagline.** A short phrase like "impermanent art" or "art of impermanence" would anchor
  the meaning for all audiences before anyone needs to look up the Japanese.
- **Avoid bare 無常 in branding.** When characters are displayed (e.g., on an about page or logo),
  prefer either the full 諸行無常 or the romanized "mujou." The bare two-character 無常 is the
  form most likely to trigger the Chinese folk-religion reading.

## Candidates

Preferred candidates are marked with `>>`.
Domain columns show the domain if available, `premium` with price if premium, `-` if taken, `?` if unchecked.

| | Name | Meaning | crates.io | .art | .garden |
| --- | --- | --- | --- | --- | --- |
| **>>** | **CRATE + DOMAIN AVAILABLE** | | | | |
| >> | samon | 砂紋, sand pattern | available | premium $81.90 | samon.garden |
| >> | sandrake | sand + rake | available | sandrake.art | sandrake.garden |
| >> | driftline | drift + line | available | driftline.art | driftline.garden |
| >> | sandrift | sand + drift | available | sandrift.art | sandrift.garden |
| >> | striate | English: to mark with lines | available | striate.art | striate.garden |
| >> | striae | plural of stria: grooves/lines | available | premium $81.90 | striae.garden |
| >> | nagisen | 凪線, calm + line | available | nagisen.art | nagisen.garden |
| >> | suji | 筋, line/path/sinew | available | premium $159.90 | suji.garden |
| >> | mujou | 無常, impermanence | available | mujou.art | mujou.garden |
| >> | karesansui | 枯山水, dry landscape zen garden | available | karesansui.art | karesansui.garden |
| >> | houkime | 箒目, rake/broom marks | available | houkime.art | houkime.garden |
| | zentrace | zen + trace | available | zentrace.art | zentrace.garden |
| | zenscribe | zen + scribe | available | zenscribe.art | zenscribe.garden |
| | zenrake | zen + rake | available | zenrake.art | zenrake.garden |
| | sandtrace | sand + trace | available | sandtrace.art | sandtrace.garden |
| | sandetch | sand + etch | available | sandetch.art | sandetch.garden |
| | sandwalk | sand + walk | available | sandwalk.art | sandwalk.garden |
| | sandcomb | combing sand | available | sandcomb.art | sandcomb.garden |
| | sandstroke | brushstroke in sand | available | sandstroke.art | sandstroke.garden |
| | sandline | sand + line | available | sandline.art | sandline.garden |
| | stillpath | still + path | available | stillpath.art | stillpath.garden |
| | tideline | tide + line | available | premium $159.90 | tideline.garden |
| | rakeline | rake + line | available | rakeline.art | rakeline.garden |
| | dunescribe | dune + scribe | available | dunescribe.art | dunescribe.garden |
| | duneline | dune + line | available | duneline.art | duneline.garden |
| | ebbline | ebb + line | available | ebbline.art | ebbline.garden |
| | nagiline | nagi + line | available | nagiline.art | nagiline.garden |
| | mosspath | quiet overgrown trail | available | mosspath.art | mosspath.garden |
| | fernline | fern frond line | available | fernline.art | fernline.garden |
| | mistline | line from mist | available | mistline.art | mistline.garden |
| | stillmoss | stillness + moss | available | stillmoss.art | stillmoss.garden |
| | rastrace | raster + trace | available | rastrace.art | rastrace.garden |
| | edgetrace | edge + trace | available | edgetrace.art | edgetrace.garden |
| | contrace | contour + trace | available | contrace.art | contrace.garden |
| | pathscribe | path + scribe | available | pathscribe.art | pathscribe.garden |
| | etchline | etch + line | available | etchline.art | etchline.garden |
| | linewalk | line + walk | available | linewalk.art | linewalk.garden |
| | tracework | traced patterns | available | tracework.art | tracework.garden |
| | etchwork | etched patterns | available | etchwork.art | etchwork.garden |
| | threadline | fine continuous line | available | threadline.art | threadline.garden |
| | filament | a fine thread or fiber | available | premium $354.90 | - |
| | lineament | a distinctive feature/line | available | lineament.art | lineament.garden |
| | graven | engraved (archaic/poetic) | available | premium $354.90 | graven.garden |
| | etching | the act/result of etching | available | premium $354.90 | etching.garden |
| | driftmark | mark left by movement | available | driftmark.art | driftmark.garden |
| | tsuyu | 露, dewdrop | available | premium $81.90 | tsuyu.garden |
| | nagisa | 渚, water's edge | available | premium $159.90 | nagisa.garden |
| | sazanami | さざ波, gentle ripples | available | premium $81.90 | sazanami.garden |
| | thero | working name | available | ? | ? |
| | scribit | write (Latin) | available | - | ? |
| | sandscribe | sand + scribe | available | - | ? |
| | zenline | zen + line | available | pending delete | ? |
| | tidemark | tide + mark | available | - | ? |
| | kaze | 風, wind | available | - | ? |
| | kiri | 霧, mist/fog | available | - | ? |
| | komichi | 小道, small path | available | - | ? |
| | ato | 跡, trace/mark | available | - | ? |
| | kasumi | 霞, haze/mist | available | - | ? |
| | wabi | 侘, rustic simplicity | available | - | ? |
| | musubi | 結び, connection/tying | available | - | ? |
| | michi | 道, path/way | available | - | ? |
| | rootline | root + line | available | - | ? |
| | trellis | lattice framework | available | - | - |
| | scribe | one who writes | available | - | ? |
| **>>** | **CRATE AVAILABLE, NO DOMAINS** | | | | |
| | *(none confirmed -- names above with `-` for .art may have .garden unchecked)* | | | | |
| **>>** | **CRATE TAKEN** | | | | |
| >> | nagi | 凪, calm/still sea | taken | ? | ? |
| | mushin | 無心, no-mind (zen) | taken | ? | ? |
| | retrace | re + trace | taken | ? | ? |
| | pathify | path + -ify | taken | ? | ? |
| | furrow | to plow lines | taken | ? | ? |
| | nagare | 流れ, flow | taken | ? | ? |
| | shizuku | 雫, droplet | taken | ? | ? |
| | tracery | decorative interlacing lines | taken | ? | ? |
| | meander | a winding path | taken | ? | ? |
| | filigree | ornamental fine wire work | taken | ? | ? |
| | rivulet | a small stream | taken | ? | ? |
| | patina | surface weathering/beauty | taken | ? | ? |
| | ridgeline | ridge + line | taken | ? | ? |
| | cairn | stacked stones | taken | ? | ? |
| | glade | forest clearing | taken | ? | ? |
| | inscribe | to engrave/write into | taken | - | ? |
| | enso | 円相, zen circle | taken | - | ? |
| | yugen | 幽玄, mysterious depth | taken | - | ? |
| **>>** | **REJECTED** | | | | |
| | *(none)* | | | | |

### Pricing notes

Prices from Namecheap Beast Mode, checked 2026-02-07.

- **Standard .art**: $3.98/yr initial, renews at $25.98/yr
- **Standard .garden**: $2.48/yr initial, renews at $30.98/yr
- **Premium .art**: varies ($81.90 - $354.90 initial), all renew at $27.30/yr
- Prices are introductory/promotional and subject to change
- Checked 2026-02-07 via Namecheap Beast Mode; all 48 fully-checked names returned results for all 3 TLDs

## Availability Notes

- crates.io checked via API (404 = available, 200 = taken)
- Domain availability and pricing checked via Namecheap Beast Mode
- `?` = not yet checked for that TLD
- `-` = taken
- Last checked: 2026-02-07
