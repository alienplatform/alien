import { hash } from "bcrypt"
import { drizzle } from "drizzle-orm/node-postgres"
import { Pool } from "pg"
import * as schema from "../lib/schema"

const pool = new Pool({
  connectionString:
    process.env.DATABASE_URL || "postgresql://postgres:postgres@localhost:5435/github_agent",
})

const db = drizzle(pool, { schema })

async function seed() {
  console.log("Seeding database...")

  // Create demo user
  const passwordHash = await hash("demo1234", 10)

  const existingUser = await db.query.user.findFirst({
    where: (user, { eq }) => eq(user.email, "demo@example.com"),
  })

  if (!existingUser) {
    await db.insert(schema.account).values({
      id: crypto.randomUUID(),
      accountId: "demo-account",
      providerId: "credential",
      userId: "demo-user",
      password: passwordHash,
      createdAt: new Date(),
      updatedAt: new Date(),
    })

    await db.insert(schema.user).values({
      id: "demo-user",
      name: "Demo User",
      email: "demo@example.com",
      emailVerified: true,
      createdAt: new Date(),
      updatedAt: new Date(),
    })

    console.log("✓ Created demo user (demo@example.com / demo1234)")
  } else {
    console.log("✓ Demo user already exists")
  }

  console.log("Seeding complete!")
  process.exit(0)
}

seed().catch(error => {
  console.error("Error seeding database:", error)
  process.exit(1)
})
