# RenderOperatorManifestRequestFormat

raw: a kubectl-applyable manifest for one cluster. helm: a paste-into-your-chart template whose namespace and environment name come from Helm at install time.

## Example Usage

```typescript
import { RenderOperatorManifestRequestFormat } from "@alienplatform/platform-api/models";

let value: RenderOperatorManifestRequestFormat = "helm";
```

## Values

```typescript
"raw" | "helm"
```