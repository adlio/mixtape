#!/usr/bin/env bash
#
# Deploy a mixtape agent to AWS Bedrock AgentCore.
#
# This script automates the full deployment pipeline:
#   1. Authenticates with ECR
#   2. Builds an ARM64 Docker image
#   3. Pushes to ECR
#   4. Creates (or updates) an AgentCore runtime
#
# Prerequisites:
#   - AWS CLI v2 configured with credentials
#   - Docker with buildx support (Docker Desktop or standalone buildx)
#   - jq for JSON parsing
#
# Usage:
#   ./mixtape-server/deploy-agentcore.sh [OPTIONS]
#
# Options:
#   --name NAME        Agent runtime name (default: mixtape-agent)
#   --region REGION    AWS region (default: us-west-2)
#   --role-arn ARN     IAM role ARN for the runtime (required on first deploy)
#   --binary NAME      Binary/example name to build (default: agentcore_server)
#   --local            Build and run locally instead of deploying
#   --help             Show this help message
#
# Examples:
#   # First deployment
#   ./mixtape-server/deploy-agentcore.sh \
#     --name my-agent \
#     --role-arn arn:aws:iam::123456789012:role/AgentCoreRole
#
#   # Subsequent deployments (updates the existing runtime)
#   ./mixtape-server/deploy-agentcore.sh --name my-agent
#
#   # Local testing
#   ./mixtape-server/deploy-agentcore.sh --local

set -euo pipefail

# Defaults
AGENT_NAME="mixtape-agent"
REGION="${AWS_REGION:-us-west-2}"
ROLE_ARN=""
BINARY_NAME="agentcore_server"
LOCAL_MODE=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --name)      AGENT_NAME="$2"; shift 2 ;;
        --region)    REGION="$2"; shift 2 ;;
        --role-arn)  ROLE_ARN="$2"; shift 2 ;;
        --binary)    BINARY_NAME="$2"; shift 2 ;;
        --local)     LOCAL_MODE=true; shift ;;
        --help)
            head -35 "$0" | tail -30
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

info()  { echo -e "${BLUE}[info]${NC}  $*"; }
ok()    { echo -e "${GREEN}[ok]${NC}    $*"; }
error() { echo -e "${RED}[error]${NC} $*" >&2; }

# ---------- Local mode ----------
if $LOCAL_MODE; then
    info "Building and running locally..."
    docker build \
        -f mixtape-server/Dockerfile.agentcore \
        --build-arg BINARY_NAME="$BINARY_NAME" \
        -t "$AGENT_NAME" .
    ok "Image built: $AGENT_NAME"

    info "Starting on port 8080..."
    echo ""
    echo "  Health check: curl http://localhost:8080/ping"
    echo "  Invoke:       curl -X POST http://localhost:8080/invocations -H 'Content-Type: application/json' -d '{\"prompt\": \"Hello\"}' -N"
    echo ""
    docker run --rm -p 8080:8080 \
        -e AWS_REGION="$REGION" \
        -e AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-}" \
        -e AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-}" \
        -e AWS_SESSION_TOKEN="${AWS_SESSION_TOKEN:-}" \
        "$AGENT_NAME"
    exit 0
fi

# ---------- Cloud deployment ----------

# Get AWS account ID
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text 2>/dev/null) || {
    error "Failed to get AWS account ID. Is the AWS CLI configured?"
    exit 1
}
info "AWS Account: $ACCOUNT_ID"

ECR_REPO="${ACCOUNT_ID}.dkr.ecr.${REGION}.amazonaws.com/${AGENT_NAME}"

# Step 1: Create ECR repository if it doesn't exist
info "Ensuring ECR repository exists..."
aws ecr describe-repositories --repository-names "$AGENT_NAME" --region "$REGION" &>/dev/null || \
    aws ecr create-repository --repository-name "$AGENT_NAME" --region "$REGION" --image-scanning-configuration scanOnPush=true &>/dev/null
ok "ECR repository: $ECR_REPO"

# Step 2: Authenticate Docker with ECR
info "Authenticating Docker with ECR..."
aws ecr get-login-password --region "$REGION" | \
    docker login --username AWS --password-stdin "${ACCOUNT_ID}.dkr.ecr.${REGION}.amazonaws.com" 2>/dev/null
ok "Docker authenticated with ECR"

# Step 3: Build and push ARM64 image
info "Building ARM64 image (this may take a few minutes on first build)..."
docker buildx build --platform linux/arm64 \
    -f mixtape-server/Dockerfile.agentcore \
    --build-arg BINARY_NAME="$BINARY_NAME" \
    -t "${ECR_REPO}:latest" \
    --push .
ok "Image pushed to ${ECR_REPO}:latest"

# Step 4: Create or update AgentCore runtime
info "Deploying to AgentCore..."

# Check if runtime already exists
EXISTING_ARN=$(aws bedrock-agentcore-control list-agent-runtimes --region "$REGION" \
    --query "agentRuntimeSummaries[?agentRuntimeName=='${AGENT_NAME}'].agentRuntimeArn | [0]" \
    --output text 2>/dev/null) || true

if [[ -n "$EXISTING_ARN" && "$EXISTING_ARN" != "None" ]]; then
    # Update existing runtime
    info "Updating existing runtime: $EXISTING_ARN"
    aws bedrock-agentcore-control update-agent-runtime \
        --agent-runtime-id "$EXISTING_ARN" \
        --agent-runtime-artifact "{\"containerConfiguration\":{\"containerUri\":\"${ECR_REPO}:latest\"}}" \
        --region "$REGION" &>/dev/null
    RUNTIME_ARN="$EXISTING_ARN"
    ok "Runtime updated"
else
    # Create new runtime
    if [[ -z "$ROLE_ARN" ]]; then
        error "First deployment requires --role-arn"
        echo ""
        echo "Create an IAM role with the following trust policy:"
        echo '  {"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"bedrock-agentcore.amazonaws.com"},"Action":"sts:AssumeRole"}]}'
        echo ""
        echo "Then run:"
        echo "  $0 --name $AGENT_NAME --role-arn <your-role-arn>"
        exit 1
    fi

    info "Creating new AgentCore runtime..."
    RESULT=$(aws bedrock-agentcore-control create-agent-runtime \
        --agent-runtime-name "$AGENT_NAME" \
        --agent-runtime-artifact "{\"containerConfiguration\":{\"containerUri\":\"${ECR_REPO}:latest\"}}" \
        --network-configuration '{"networkMode":"PUBLIC"}' \
        --role-arn "$ROLE_ARN" \
        --region "$REGION" \
        --output json)
    RUNTIME_ARN=$(echo "$RESULT" | jq -r '.agentRuntimeArn')
    ok "Runtime created: $RUNTIME_ARN"
fi

echo ""
echo "=========================================="
echo "  Deployment complete!"
echo "=========================================="
echo ""
echo "  Runtime:  $AGENT_NAME"
echo "  ARN:      ${RUNTIME_ARN}"
echo "  Region:   $REGION"
echo ""
echo "  Invoke with:"
echo "    aws bedrock-agentcore invoke-agent-runtime \\"
echo "      --agent-runtime-arn ${RUNTIME_ARN} \\"
echo "      --payload '{\"prompt\": \"Hello!\"}' \\"
echo "      --content-type application/json \\"
echo "      --region $REGION"
echo ""
