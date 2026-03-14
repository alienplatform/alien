# ManagementPermissionsEnum

Auto-derived permissions only (default)
Uses resource lifecycles to determine management permissions:
- Frozen resources: `<type>/management`
- Live/LiveOnSetup resources: `<type>/provision`

## Example Usage

```typescript
import { ManagementPermissionsEnum } from "@alienplatform/manager-api/models";

let value: ManagementPermissionsEnum = "auto";
```

## Values

```typescript
"auto"
```