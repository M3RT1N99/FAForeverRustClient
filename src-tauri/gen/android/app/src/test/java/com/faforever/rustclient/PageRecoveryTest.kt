package com.faforever.rustclient

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

// The page's way back once its renderer has ended (PageRecovery in
// MainActivity.kt): at once in front, and from the back only once the
// activity is in front again, once. Whether it comes back at all is the
// core's answer, whose cap counts only rebuilds in front
// (src-tauri/src/recovery_cap.rs has that half). An end from the back that
// reaches the client only just after it is in front again is still one in
// the back. Times in milliseconds, as SystemClock.elapsedRealtime's.
class PageRecoveryTest {
  @Test
  fun aPageGoneInTheBackWaitsForTheFront() {
    val page = PageRecovery()
    assertEquals(PageAnswer.IN_FRONT, page.rendererGone(inFront = false, comesBack = true))
    assertTrue(page.inFrontAgain(now = 60_000))
    // Built once: the next return finds nothing waiting.
    assertFalse(page.inFrontAgain(now = 120_000))
  }

  @Test
  fun aPageGoneInFrontIsBuiltAtOnceAndLeavesNothingWaiting() {
    val page = PageRecovery()
    assertEquals(PageAnswer.NOW, page.rendererGone(inFront = true, comesBack = true))
    assertFalse(page.inFrontAgain(now = 60_000))
  }

  @Test
  fun aReturnWithoutAnEndBuildsNothing() {
    assertFalse(PageRecovery().inFrontAgain(now = 60_000))
  }

  @Test
  fun pastTheCapTheEndIsAndroids() {
    val page = PageRecovery()
    assertEquals(PageAnswer.ANDROID, page.rendererGone(inFront = true, comesBack = false))
    assertEquals(PageAnswer.ANDROID, page.rendererGone(inFront = false, comesBack = false))
    assertFalse(page.inFrontAgain(now = 60_000))
  }

  @Test
  fun endsInTheBackWaitForOneBuildInFront() {
    val page = PageRecovery()
    page.rendererGone(inFront = false, comesBack = true)
    page.rendererGone(inFront = false, comesBack = true)
    assertTrue(page.inFrontAgain(now = 60_000))
    assertFalse(page.inFrontAgain(now = 120_000))
  }

  @Test
  fun anEndFollowsTheActivityAwayFromAReturn() {
    val page = PageRecovery()
    // Before any return, as after a start.
    assertTrue(page.endedInFront(started = true, crashed = false, now = 5_000))
    assertFalse(page.endedInFront(started = false, crashed = false, now = 5_000))
    assertFalse(page.endedInFront(started = false, crashed = true, now = 5_000))
    page.inFrontAgain(now = 60_000)
    // Past a second after the return, an end in front is one.
    assertTrue(page.endedInFront(started = true, crashed = false, now = 61_001))
    // In the back again, whenever.
    assertFalse(page.endedInFront(started = false, crashed = false, now = 60_500))
  }

  @Test
  fun anEndFromTheBackThatComesJustAfterTheReturnIsInTheBack() {
    val page = PageRecovery()
    page.inFrontAgain(now = 60_000)
    // Delivered as the frozen client thaws, the activity started by then.
    assertFalse(page.endedInFront(started = true, crashed = false, now = 60_000))
    assertFalse(page.endedInFront(started = true, crashed = false, now = 61_000))
    // So it is not built at once, and the next return builds it.
    assertEquals(PageAnswer.IN_FRONT, page.rendererGone(inFront = false, comesBack = true))
    assertTrue(page.inFrontAgain(now = 300_000))
  }

  @Test
  fun aCrashJustAfterTheReturnIsInFront() {
    val page = PageRecovery()
    page.inFrontAgain(now = 60_000)
    assertTrue(page.endedInFront(started = true, crashed = true, now = 60_200))
  }

  @Test
  fun eachReturnStartsTheSecondAgain() {
    val page = PageRecovery()
    page.inFrontAgain(now = 60_000)
    page.inFrontAgain(now = 120_000)
    assertFalse(page.endedInFront(started = true, crashed = false, now = 120_500))
  }
}
