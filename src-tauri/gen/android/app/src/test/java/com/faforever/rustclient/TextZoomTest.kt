package com.faforever.rustclient

import org.junit.Assert.assertEquals
import org.junit.Test

// The webview's text zoom for the system's font size (textZoomFor in
// MainActivity.kt): the font size in percent, a smaller one followed all the
// way down, a larger one held at 1.3.
class TextZoomTest {
  @Test
  fun followsTheFontSize() {
    assertEquals(85, textZoomFor(0.85f))
    assertEquals(100, textZoomFor(1.0f))
    assertEquals(115, textZoomFor(1.15f))
    assertEquals(130, textZoomFor(1.3f))
  }

  @Test
  fun roundsToTheNearestPercent() {
    // Cut off, 1.149 would be 114; rounded up, 1.144 would be 115.
    assertEquals(115, textZoomFor(1.149f))
    assertEquals(114, textZoomFor(1.144f))
  }

  @Test
  fun holdsALargerFontSizeAtTheCap() {
    assertEquals(130, textZoomFor(2.0f))
  }
}
