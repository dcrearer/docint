# Adding New Users

Self sign-up is disabled for security. Administrators must create user accounts manually.

## Method 1: AWS CLI (Recommended)

```bash
# Get the User Pool ID from CDK outputs
USER_POOL_ID=$(aws cloudformation describe-stacks \
  --stack-name DocintAuthStack \
  --query "Stacks[0].Outputs[?OutputKey=='UserPoolId'].OutputValue" \
  --output text)

# Create a new user
aws cognito-idp admin-create-user \
  --user-pool-id "$USER_POOL_ID" \
  --username alice \
  --temporary-password "TempPass123!" \
  --user-attributes Name=email,Value=alice@example.com \
  --message-action SUPPRESS  # Don't send email

# User must change password on first login
```

## Method 2: AWS Console

1. Navigate to **Amazon Cognito** in AWS Console
2. Click **User pools** → **docint-users**
3. Click **Create user**
4. Fill in:
   - Username: `alice`
   - Email: `alice@example.com` (optional)
   - Temporary password: `TempPass123!`
   - Uncheck "Send an email invitation"
5. Click **Create user**

## First Login Flow

When the user runs the CLI for the first time:

```bash
export DOCINT_RUNTIME_ARN="arn:aws:bedrock-agentcore:us-east-1:<ACCOUNT_ID>:runtime/<RUNTIME_ID>"
export DOCINT_CLIENT_ID="<COGNITO_CLIENT_ID>"

docint-cli
```

They'll be prompted:
1. Username: `alice`
2. Password: `TempPass123!` (temporary)
3. New password: (user sets their permanent password)
4. Confirm password: (confirm)

## User Management

### List all users
```bash
aws cognito-idp list-users \
  --user-pool-id "$USER_POOL_ID" \
  --query "Users[*].[Username,UserStatus,Enabled]" \
  --output table
```

### Disable a user
```bash
aws cognito-idp admin-disable-user \
  --user-pool-id "$USER_POOL_ID" \
  --username alice
```

### Enable a user
```bash
aws cognito-idp admin-enable-user \
  --user-pool-id "$USER_POOL_ID" \
  --username alice
```

### Delete a user
```bash
aws cognito-idp admin-delete-user \
  --user-pool-id "$USER_POOL_ID" \
  --username alice
```

### Reset user password
```bash
aws cognito-idp admin-set-user-password \
  --user-pool-id "$USER_POOL_ID" \
  --username alice \
  --password "NewTempPass123!" \
  --permanent  # Or omit for temporary password
```

## Security Notes

- Each user gets a unique `tenant_id` (Cognito `sub` UUID)
- RLS enforces complete data isolation between tenants
- Users cannot see or access other users' documents
- Temporary passwords must be changed on first login
- Password policy: min 8 chars, lowercase, digits required
