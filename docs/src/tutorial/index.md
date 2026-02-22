# Tutorial: Convert a Photo to a Sand Table Pattern

This tutorial walks through converting a photograph into a single
continuous path that a kinetic sand table can trace.  By the end you
will have a `.thr` file ready to upload to your table.

All processing happens in your browser -- no images leave your device.

---

## 1. Open the app

When you first open mujou the bundled example image (cherry blossoms)
is already processed and the **Output** stage is selected.  The main
preview shows the final path clipped to a circular canvas, the
filmstrip along the bottom shows thumbnails for each pipeline stage,
and the controls panel below offers per-stage parameters.

To use your own image, click the
{{#include images/icon-upload.html}}
upload button at the top of the page, or drag and drop a file anywhere.
PNG, JPEG, BMP, and WebP are supported.

<div class="screenshot">
  <img class="screenshot-light" src="images/01-landing-light.png" alt="mujou landing page showing the Output stage with a cherry blossom pattern clipped to a circle">
  <img class="screenshot-dark"  src="images/01-landing-dark.png"  alt="mujou landing page showing the Output stage with a cherry blossom pattern clipped to a circle">
</div>

---

## 2. View the original photo

Click the **Original** thumbnail in the filmstrip to see the source
image.  This is the unmodified photo that the pipeline starts from.

<div class="screenshot">
  <img class="screenshot-light" src="images/02-original-light.png" alt="Original stage showing the cherry blossom photograph">
  <img class="screenshot-dark"  src="images/02-original-dark.png"  alt="Original stage showing the cherry blossom photograph">
</div>

---

## 3. Tune the edge detection

Click the **Edges** thumbnail.  The Canny edge detector finds the
outlines in your image -- these are the lines the sand table will
trace.  Below the preview you can see the Canny threshold sliders,
an **Invert** toggle, and **Edge Channels** checkboxes.

The three threshold sliders control which edges are kept:

- **Canny Low** -- minimum gradient strength to *consider* a pixel as
  a potential edge.
- **Canny High** -- gradient strength above which a pixel is
  *definitely* an edge.
- **Canny Max** -- the maximum possible gradient value (normalizes
  the scale).

Try lowering **Canny Low** (here set to 5, down from the default of
15) to keep weaker edges and capture more detail.  The pipeline
reprocesses automatically after each change.

<div class="screenshot">
  <img class="screenshot-light" src="images/03-edges-light.png" alt="Edges stage with Canny threshold controls showing adjusted Canny Low">
  <img class="screenshot-dark"  src="images/03-edges-dark.png"  alt="Edges stage with Canny threshold controls showing adjusted Canny Low">
</div>

---

## 4. View the joined path

Click the **Join** thumbnail.  This is where the magic happens: the
MST (Minimum Spanning Tree) joiner connects all the separate edge
contours into a single continuous path.  A sand table ball cannot be
"lifted," so the entire output must be one unbroken line.

The **Join Controls** panel offers options for the joining strategy,
start point, MST neighbour count, and parity strategy.

<div class="screenshot">
  <img class="screenshot-light" src="images/04-join-light.png" alt="Join stage showing the single continuous path with MST join controls">
  <img class="screenshot-dark"  src="images/04-join-dark.png"  alt="Join stage showing the single continuous path with MST join controls">
</div>

---

## 5. Inspect the join diagnostics

While viewing the Join stage, click the
{{#include images/icon-layers.html}}
**diagnostic overlay** button on the left side.  The overlay color-codes
the connections between contours so you can see exactly how the paths
were joined:

- **Red dots** mark endpoints of original contours.
- **Colored segments** (orange, blue, green) show the connecting
  paths added by the joiner.
- **Green circle** marks the start point of the path.

<div class="screenshot">
  <img class="screenshot-light" src="images/05-join-diagnostics-light.png" alt="Join stage with diagnostic overlay showing colored connections between contours">
  <img class="screenshot-dark"  src="images/05-join-diagnostics-dark.png"  alt="Join stage with diagnostic overlay showing colored connections between contours">
</div>

---

## 6. View the final output

Click the **Output** thumbnail to see the finished path.  This stage
applies subsampling (subdividing long straight segments into shorter
ones) so the path renders smoothly when converted to the polar
coordinate system used by THR files.

<div class="screenshot">
  <img class="screenshot-light" src="images/06-output-light.png" alt="Output stage showing the final path ready for export">
  <img class="screenshot-dark"  src="images/06-output-dark.png"  alt="Output stage showing the final path ready for export">
</div>

---

## 7. Export to THR

Click the
{{#include images/icon-download.html}}
**export** button at the top of the page.  In the Export dialog:

1. Check **THR** (it should be checked by default).
2. Click **Download**.

The browser will download a `.thr` file containing your pattern in
polar coordinates.

<div class="screenshot">
  <img class="screenshot-light" src="images/07-export-light.png" alt="Export dialog with THR format selected and Download button">
  <img class="screenshot-dark"  src="images/07-export-dark.png"  alt="Export dialog with THR format selected and Download button">
</div>

---

## 8. Load onto your table

Upload the downloaded file to your sand table:

| Table | Format | How to upload |
| ----- | ------ | ------------- |
| **Sisyphus** | THR | Upload via the [Sisyphus app](https://sisyphus-industries.com/) or the [Web Center](https://webcenter.sisyphus-industries.com/). |
| **Oasis** | THR | Upload at [app.grounded.so](https://app.grounded.so). |
| **Dune Weaver** | THR | Upload via your table's [web UI](https://github.com/tuanchris/dune-weaver). |
