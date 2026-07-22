import { ai } from "@alienplatform/sdk"

// The gateway exposes the cloud's curated model set; getAvailableModels reads it
// so the UI can offer a picker without hardcoding model ids.
export async function GET() {
  const models = await ai("llm").getAvailableModels()
  return Response.json({ models: models.map(m => m.id) })
}
