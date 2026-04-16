# GenerateDeepstoreTokenRequest

## Example Usage

```typescript
import { GenerateDeepstoreTokenRequest } from "@alienplatform/platform-api/models";

let value: GenerateDeepstoreTokenRequest = {};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `project`                                                                                                                      | *string*                                                                                                                       | :heavy_minus_sign:                                                                                                             | Project ID or name to scope token access to. When omitted, the token is scoped to all projects accessible by the current user. |