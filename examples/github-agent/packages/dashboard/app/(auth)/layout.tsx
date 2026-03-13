export default function AuthLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <div className="min-h-svh flex items-center justify-center bg-gradient-to-br from-background via-background to-muted/30">
      {children}
    </div>
  )
}
