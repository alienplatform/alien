"use client"

import { useCallback, useEffect, useImperativeHandle, useRef } from "react"
import confetti from "canvas-confetti"
import type { Options as ConfettiOptions } from "canvas-confetti"

export interface ConfettiRef {
  fire: (options?: ConfettiOptions) => void
}

interface ConfettiProps {
  options?: ConfettiOptions
  className?: string
  onMouseEnter?: () => void
}

interface ConfettiButtonProps {
  options?: ConfettiOptions
  children?: React.ReactNode
  className?: string
}

export const Confetti = ({
  options,
  className = "",
  onMouseEnter,
  ref,
}: ConfettiProps & { ref?: React.Ref<ConfettiRef> }) => {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const confettiInstance = useRef<confetti.CreateTypes | null>(null)

  useEffect(() => {
    if (canvasRef.current) {
      confettiInstance.current = confetti.create(canvasRef.current, {
        resize: true,
        useWorker: true,
      })
    }

    return () => {
      if (confettiInstance.current) {
        confettiInstance.current.reset()
      }
    }
  }, [])

  const makeShot = useCallback(
    (opts?: ConfettiOptions) => {
      if (confettiInstance.current) {
        confettiInstance.current({
          ...options,
          ...opts,
        })
      }
    },
    [options]
  )

  useImperativeHandle(
    ref,
    () => ({
      fire: makeShot,
    }),
    [makeShot]
  )

  return (
    <canvas
      ref={canvasRef}
      className={className}
      onMouseEnter={onMouseEnter}
    />
  )
}

export function ConfettiButton({
  options,
  children,
  className = "",
}: ConfettiButtonProps) {
  const confettiRef = useRef<ConfettiRef>(null)

  const handleClick = () => {
    confettiRef.current?.fire(options || {})
  }

  return (
    <>
      <button onClick={handleClick} className={className}>
        {children}
      </button>
      <Confetti
        ref={confettiRef}
        className="pointer-events-none fixed left-0 top-0 z-50 size-full"
      />
    </>
  )
}

