#!/bin/bash

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Function to show usage
show_usage() {
    echo "Usage: $0 [patch|minor|major|<specific_version>]"
    echo ""
    echo "Examples:"
    echo "  $0 patch          # Bump patch version (0.1.100 -> 0.1.101)"
    echo "  $0 minor          # Bump minor version (0.1.100 -> 0.2.0)"
    echo "  $0 major          # Bump major version (0.1.100 -> 1.0.0)"
    echo "  $0 1.2.3          # Set specific version to 1.2.3"
    echo "  $0                # Interactive mode - will prompt for version type"
}

# Function to get current version from Cargo.toml
get_current_version() {
    grep '^version = ' Cargo.toml | sed -E 's/version = "([^"]+)"/\1/'
}

# Function to validate semantic version format
validate_version() {
    local version=$1
    if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        print_error "Invalid version format: $version. Expected format: X.Y.Z"
        return 1
    fi
    return 0
}

# Function to bump version
bump_version() {
    local current_version=$1
    local bump_type=$2
    
    IFS='.' read -ra VERSION_PARTS <<< "$current_version"
    local major=${VERSION_PARTS[0]}
    local minor=${VERSION_PARTS[1]}
    local patch=${VERSION_PARTS[2]}
    
    case $bump_type in
        "patch")
            patch=$((patch + 1))
            ;;
        "minor")
            minor=$((minor + 1))
            patch=0
            ;;
        "major")
            major=$((major + 1))
            minor=0
            patch=0
            ;;
        *)
            print_error "Invalid bump type: $bump_type"
            return 1
            ;;
    esac
    
    echo "$major.$minor.$patch"
}

# Function to update version in Cargo.toml
update_cargo_version() {
    local new_version=$1
    local temp_file=$(mktemp)
    
    # Update the version line in Cargo.toml
    sed "s/^version = \".*\"/version = \"$new_version\"/" Cargo.toml > "$temp_file"
    mv "$temp_file" Cargo.toml
    
    print_success "Updated Cargo.toml version to $new_version"
}

# Function to check if git working directory is clean
check_git_status() {
    if [[ -n $(git status --porcelain) ]]; then
        print_warning "Working directory has uncommitted changes."
        echo "The following files will be included in the release commit:"
        git status --short
        echo ""
        read -p "Do you want to continue? (y/N): " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            print_info "Release cancelled."
            exit 0
        fi
    fi
}

# Function to commit and push changes
commit_and_push() {
    local version=$1
    
    print_info "Adding changes to git..."
    git add Cargo.toml
    
    # Add any other uncommitted changes if they exist
    if [[ -n $(git status --porcelain) ]]; then
        git add .
    fi
    
    print_info "Committing version bump..."
    git commit -m "chore: bump version to $version"
    
    print_info "Pushing changes to remote..."
    git push origin $(git branch --show-current)
    
    print_success "Changes committed and pushed"
}

# Function to create and push git tag
create_and_push_tag() {
    local version=$1
    local tag="v$version"
    
    print_info "Creating git tag: $tag"
    git tag "$tag"
    
    print_info "Pushing tag to remote..."
    git push --tags
    
    print_success "Tag $tag created and pushed"
}

# Main script logic
main() {
    print_info "Starting release process..."
    
    # Check if we're in a git repository
    if ! git rev-parse --git-dir > /dev/null 2>&1; then
        print_error "Not in a git repository"
        exit 1
    fi
    
    # Check if Cargo.toml exists
    if [[ ! -f "Cargo.toml" ]]; then
        print_error "Cargo.toml not found in current directory"
        exit 1
    fi
    
    # Get current version
    current_version=$(get_current_version)
    if [[ -z "$current_version" ]]; then
        print_error "Could not find version in Cargo.toml"
        exit 1
    fi
    
    print_info "Current version: $current_version"
    
    # Determine new version
    local new_version
    local version_input="$1"
    
    if [[ -z "$version_input" ]]; then
        # Interactive mode
        echo ""
        echo "Select version bump type:"
        echo "1) patch (${current_version} -> $(bump_version "$current_version" "patch"))"
        echo "2) minor (${current_version} -> $(bump_version "$current_version" "minor"))"
        echo "3) major (${current_version} -> $(bump_version "$current_version" "major"))"
        echo "4) custom (specify exact version)"
        echo ""
        read -p "Enter choice (1-4): " -n 1 -r choice
        echo ""
        
        case $choice in
            1) new_version=$(bump_version "$current_version" "patch") ;;
            2) new_version=$(bump_version "$current_version" "minor") ;;
            3) new_version=$(bump_version "$current_version" "major") ;;
            4) 
                read -p "Enter custom version (X.Y.Z format): " custom_version
                if validate_version "$custom_version"; then
                    new_version="$custom_version"
                else
                    exit 1
                fi
                ;;
            *)
                print_error "Invalid choice"
                exit 1
                ;;
        esac
    elif [[ "$version_input" == "patch" || "$version_input" == "minor" || "$version_input" == "major" ]]; then
        # Bump version based on type
        new_version=$(bump_version "$current_version" "$version_input")
    elif validate_version "$version_input"; then
        # Specific version provided
        new_version="$version_input"
    else
        show_usage
        exit 1
    fi
    
    print_info "New version will be: $new_version"
    
    # Confirm the release
    echo ""
    read -p "Proceed with release $current_version -> $new_version? (y/N): " -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        print_info "Release cancelled."
        exit 0
    fi
    
    # Check git status
    check_git_status
    
    # Update version in Cargo.toml
    update_cargo_version "$new_version"
    
    # Commit and push changes
    commit_and_push "$new_version"
    
    # Create and push tag
    create_and_push_tag "$new_version"
    
    print_success "Release $new_version completed successfully! 🎉"
    print_info "You can now check your CI/CD pipeline or manually trigger any additional release processes."
}

# Handle help flag
if [[ "$1" == "-h" || "$1" == "--help" ]]; then
    show_usage
    exit 0
fi

# Run main function
main "$@"