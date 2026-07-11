# GeneratedDomain

Parent domain for generated deployment URLs. Chosen public subdomains are only allowed when isSystem is false.

## Example Usage

```typescript
import { GeneratedDomain } from "@alienplatform/platform-api/models";

let value: GeneratedDomain = {
  domain: "weekly-tail.info",
  isSystem: true,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `domain`           | *string*           | :heavy_check_mark: | N/A                |
| `isSystem`         | *boolean*          | :heavy_check_mark: | N/A                |