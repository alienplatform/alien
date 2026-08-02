import { z } from "zod"

// The model picks a question and passes bounded arguments; the SQL is written here and
// the arguments are bound, so nothing it sends reaches the database as SQL.
export const QUESTIONS = [
  "customer_count_by_plan",
  "customer_count_by_country",
  "total_mrr_by_plan",
  "top_customers_by_mrr",
  "orders_by_status",
  "recent_orders",
  "revenue_by_customer",
] as const

export const askSchema = z.object({
  question: z.enum(QUESTIONS).describe("which question to answer"),
  plan: z.enum(["enterprise", "pro", "starter"]).optional().describe("filter by plan"),
  status: z.enum(["paid", "pending", "refunded"]).optional().describe("filter by order status"),
  limit: z.number().int().min(1).max(50).default(10).describe("rows to return, at most 50"),
})

export type Ask = z.infer<typeof askSchema>

const FILTERS: Record<Ask["question"], Array<"plan" | "status">> = {
  customer_count_by_plan: ["plan"],
  customer_count_by_country: [],
  total_mrr_by_plan: ["plan"],
  top_customers_by_mrr: ["plan"],
  orders_by_status: ["status"],
  recent_orders: ["status"],
  revenue_by_customer: ["status"],
}

export function supportedFilters(question: Ask["question"]): Array<"plan" | "status"> {
  return FILTERS[question]
}

/** Filters this question ignores, so a dropped one can't be read back as applied. */
export function unsupportedFilters(ask: Ask): Array<"plan" | "status"> {
  return (["plan", "status"] as const).filter(f => ask[f] && !FILTERS[ask.question].includes(f))
}

/** The statement and bound values for one question. */
export function plan({ question, plan: planFilter, status, limit }: Ask): {
  text: string
  values: unknown[]
} {
  switch (question) {
    case "customer_count_by_plan":
      return {
        text: `select plan, count(*)::int as customers, sum(mrr_usd)::int as total_mrr
               from customers where ($1::text is null or plan = $1)
               group by plan order by total_mrr desc`,
        values: [planFilter ?? null],
      }
    case "customer_count_by_country":
      return {
        text: `select country, count(*)::int as customers, sum(mrr_usd)::int as total_mrr
               from customers group by country order by customers desc, total_mrr desc`,
        values: [],
      }
    case "total_mrr_by_plan":
      return {
        text: `select coalesce(sum(mrr_usd), 0)::int as total_mrr, count(*)::int as customers
               from customers where ($1::text is null or plan = $1)`,
        values: [planFilter ?? null],
      }
    case "top_customers_by_mrr":
      return {
        text: `select name, plan, country, mrr_usd from customers
               where ($1::text is null or plan = $1)
               order by mrr_usd desc limit $2`,
        values: [planFilter ?? null, limit],
      }
    case "orders_by_status":
      return {
        text: `select status, count(*)::int as orders, sum(amount_usd)::int as total_usd
               from orders where ($1::text is null or status = $1)
               group by status order by total_usd desc`,
        values: [status ?? null],
      }
    case "recent_orders":
      return {
        text: `select o.id, c.name as customer, o.amount_usd, o.status, o.created
               from orders o join customers c on c.id = o.customer_id
               where ($1::text is null or o.status = $1)
               order by o.created desc, o.id desc limit $2`,
        values: [status ?? null, limit],
      }
    case "revenue_by_customer":
      return {
        text: `select c.name, count(o.id)::int as orders,
                      coalesce(sum(o.amount_usd), 0)::int as total_usd
               from customers c
               left join orders o
                 on o.customer_id = c.id and ($1::text is null or o.status = $1)
               group by c.name order by total_usd desc limit $2`,
        values: [status ?? null, limit],
      }
  }
}
