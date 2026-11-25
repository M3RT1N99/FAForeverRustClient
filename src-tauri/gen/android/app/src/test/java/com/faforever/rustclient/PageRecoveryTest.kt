package com.faforever.rustclient

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

// The page's way back once its renderer has ended (PageRecovery in
// MainActivity.kt): at once in front, and from the back only once the
// activity is in front again, once. Whether it comes back at all is the
// core's answer, whose cap counts only rebuilds in front
// (src-tauri/src/recovery_cap.rs has that half).
class PageRecoveryTest {
  @Test
  fun aPageGoneInTheBackWaitsForTheFront() {
    val page = PageRecovery()
    assertEquals(PageAnswer.IN_FRONT, page.rendererGone(inFront = false, comesBack = true))
    assertTrue(page.inFrontAgain())
    // Built once: the next return finds nothing waiting.
    assertFalse(page.inFrontAgain())
  }

  @Test
  fun aPageGoneInFrontIsBuiltAtOnceAndLeavesNothingWaiting() {
    val page = PageRecovery()
    assertEquals(PageAnswer.NOW, page.rendererGone(inFront = true, comesBack = true))
    assertFalse(page.inFrontAgain())
  }

  @Test
  fun aReturnWithoutAnEndBuildsNothing() {
    assertFalse(PageRecovery().inFrontAgain())
  }

  @Test
  fun pastTheCapTheEndIsAndroids() {
    val page = PageRecovery()
    assertEquals(PageAnswer.ANDROID, page.rendererGone(inFront = true, comesBack = false))
    assertEquals(PageAnswer.ANDROID, page.rendererGone(inFront = false, comesBack = false))
    assertFalse(page.inFrontAgain())
  }

  @Test
  fun endsInTheBackWaitForOneBuildInFront() {
    val page = PageRecovery()
    page.rendererGone(inFront = false, comesBack = true)
    page.rendererGone(inFront = false, comesBack = true)
    assertTrue(page.inFrontAgain())
    assertFalse(page.inFrontAgain())
  }
}
