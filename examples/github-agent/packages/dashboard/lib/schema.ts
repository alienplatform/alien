import { pgTable, text, timestamp, boolean, integer, real } from "drizzle-orm/pg-core"

// Better Auth tables
export const user = pgTable("user", {
  id: text("id").primaryKey(),
  name: text("name").notNull(),
  email: text("email").notNull().unique(),
  emailVerified: boolean("email_verified").notNull().default(false),
  image: text("image"),
  createdAt: timestamp("created_at").notNull(),
  updatedAt: timestamp("updated_at").notNull(),
})

// Session table (includes activeOrganizationId for better-auth organization plugin)
export const session = pgTable("session", {
  id: text("id").primaryKey(),
  expiresAt: timestamp("expires_at").notNull(),
  token: text("token").notNull().unique(),
  createdAt: timestamp("created_at").notNull(),
  updatedAt: timestamp("updated_at").notNull(),
  ipAddress: text("ip_address"),
  userAgent: text("user_agent"),
  userId: text("user_id").notNull().references(() => user.id),
  activeOrganizationId: text("active_organization_id"),
})

export const account = pgTable("account", {
  id: text("id").primaryKey(),
  accountId: text("account_id").notNull(),
  providerId: text("provider_id").notNull(),
  userId: text("user_id").notNull().references(() => user.id),
  accessToken: text("access_token"),
  refreshToken: text("refresh_token"),
  idToken: text("id_token"),
  accessTokenExpiresAt: timestamp("access_token_expires_at"),
  refreshTokenExpiresAt: timestamp("refresh_token_expires_at"),
  scope: text("scope"),
  password: text("password"),
  createdAt: timestamp("created_at").notNull(),
  updatedAt: timestamp("updated_at").notNull(),
})

export const verification = pgTable("verification", {
  id: text("id").primaryKey(),
  identifier: text("identifier").notNull(),
  value: text("value").notNull(),
  expiresAt: timestamp("expires_at").notNull(),
  createdAt: timestamp("created_at"),
  updatedAt: timestamp("updated_at"),
})

// Organization tables (better-auth plugin)
export const organization = pgTable("organization", {
  id: text("id").primaryKey(),
  name: text("name").notNull(),
  slug: text("slug").unique(),
  logo: text("logo"),
  metadata: text("metadata"), // JSON string
  createdAt: timestamp("created_at").notNull(),
  updatedAt: timestamp("updated_at"), // Nullable - better-auth may not set this initially
})

export const member = pgTable("member", {
  id: text("id").primaryKey(),
  organizationId: text("organization_id").notNull().references(() => organization.id),
  userId: text("user_id").notNull().references(() => user.id),
  role: text("role").notNull(), // 'owner' | 'admin' | 'member'
  createdAt: timestamp("created_at").notNull(),
})

export const invitation = pgTable("invitation", {
  id: text("id").primaryKey(),
  organizationId: text("organization_id").notNull().references(() => organization.id),
  email: text("email").notNull(),
  role: text("role").notNull(),
  status: text("status").notNull(), // 'pending' | 'accepted' | 'rejected' | 'canceled'
  expiresAt: timestamp("expires_at").notNull(),
  inviterId: text("inviter_id").notNull().references(() => user.id),
  createdAt: timestamp("created_at").notNull(),
})

// App-specific tables

// Organization metadata - stores deployment group info for each organization
export const organizationMetadata = pgTable("organization_metadata", {
  id: text("id").primaryKey(),
  organizationId: text("organization_id").notNull().references(() => organization.id).unique(),
  deploymentGroupId: text("deployment_group_id"),
  deploymentToken: text("deployment_token"),
  createdAt: timestamp("created_at").notNull(),
  updatedAt: timestamp("updated_at").notNull(),
})

// Integrations - stores metadata about GitHub integrations (credentials stay in agent vault)
export const integration = pgTable("integration", {
  id: text("id").primaryKey(),
  organizationId: text("organization_id").notNull().references(() => organization.id),
  agentId: text("agent_id"), // The agent responsible for this integration
  owner: text("owner").notNull(),
  repo: text("repo").notNull(),
  baseUrl: text("base_url"), // For GitHub Enterprise
  hasToken: boolean("has_token").notNull().default(false),
  isActive: boolean("is_active").notNull().default(true), // False when agent is deleted
  createdAt: timestamp("created_at").notNull(),
  updatedAt: timestamp("updated_at").notNull(),
})

// Metrics history - stores aggregated metrics from periodic syncs
export const metricsHistory = pgTable("metrics_history", {
  id: text("id").primaryKey(),
  integrationId: text("integration_id").notNull().references(() => integration.id),
  totalPRs: integer("total_prs").notNull(),
  smallPRs: integer("small_prs").notNull(),
  mediumPRs: integer("medium_prs").notNull(),
  largePRs: integer("large_prs").notNull(),
  lowRiskPRs: integer("low_risk_prs").notNull(),
  mediumRiskPRs: integer("medium_risk_prs").notNull(),
  highRiskPRs: integer("high_risk_prs").notNull(),
  criticalRiskPRs: integer("critical_risk_prs").notNull(),
  avgTimeToFirstReviewHours: real("avg_time_to_first_review_hours"),
  avgMergeTimeHours: real("avg_merge_time_hours"),
  reviewThroughputScore: integer("review_throughput_score"),
  churnHotspots: text("churn_hotspots"), // JSON string
  syncedAt: timestamp("synced_at").notNull(),
})

// Sync status - tracks the last sync for each integration
export const syncStatus = pgTable("sync_status", {
  id: text("id").primaryKey(),
  integrationId: text("integration_id").notNull().references(() => integration.id).unique(),
  lastSyncAt: timestamp("last_sync_at"),
  lastSyncStatus: text("last_sync_status"), // 'success' | 'error'
  lastSyncError: text("last_sync_error"),
  nextSyncAt: timestamp("next_sync_at"),
})
