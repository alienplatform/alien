# Network


## Supported Types

### `operations.NetworkUseDefault`

```typescript
const value: operations.NetworkUseDefault = {
  type: "use-default",
};
```

### `operations.NetworkCreate`

```typescript
const value: operations.NetworkCreate = {
  type: "create",
};
```

### `operations.NetworkByoVpcAws`

```typescript
const value: operations.NetworkByoVpcAws = {
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `operations.NetworkByoVpcGcp`

```typescript
const value: operations.NetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `operations.NetworkByoVnetAzure`

```typescript
const value: operations.NetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

