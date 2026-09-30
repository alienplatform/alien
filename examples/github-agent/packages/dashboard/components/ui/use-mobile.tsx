import * as React from "react"

const MOBILE_BREAKPOINT = 768
const query = `(max-width: ${MOBILE_BREAKPOINT - 1}px)`

function subscribe(callback: () => void) {
  const mediaQuery = window.matchMedia(query)
  mediaQuery.addEventListener("change", callback)
  return () => mediaQuery.removeEventListener("change", callback)
}

function getSnapshot() {
  return window.matchMedia(query).matches
}

export function useIsMobile() {
  return React.useSyncExternalStore(subscribe, getSnapshot, () => false)
}
