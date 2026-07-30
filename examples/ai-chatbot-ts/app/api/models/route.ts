import { ai } from "@alienplatform/sdk"

export async function GET() {
  const models = await ai("llm").getAvailableModels()
  return Response.json({ models: models.map(m => m.id) })
}
