# GenerateManagerBindingTokenRequestKind

Binding kind the token resolves. Defaults to the deployment's own kind: sandbox for a sandbox deployment, data otherwise.

## Example Usage

```typescript
import { GenerateManagerBindingTokenRequestKind } from "@alienplatform/platform-api/models";

let value: GenerateManagerBindingTokenRequestKind = "data";
```

## Values

```typescript
"data" | "sandbox"
```