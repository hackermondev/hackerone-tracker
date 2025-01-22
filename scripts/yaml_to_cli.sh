#!/bin/bash

# Function to process a YAML key-value pair and convert it to CLI argument
process_yaml_key() {
  local key="$1"
  local value="$2"

  # Modify this logic as per your requirement
  # Currently, it appends `--` to the key and separates the key-value pair with an equal sign
  echo "--${key}=${value}"
}

# Function to parse the YAML file and convert keys to CLI arguments for the given app name
parse_yaml_file() {
  local file="$1"
  local config_name="$2"

  # Convert YAML to JSON using yq
  local json_output=$(yq -o=json "$file")

  # Extract the section for the given app name from the JSON output
  local section=$(echo "$json_output" | jq -r ".$config_name")

  # Extract the keys and values from the section
  local keys=$(echo "$section" | jq -r 'keys_unsorted[]')
  local values=$(echo "$section" | jq -r '.[]')
  local values_array=($values)

  # Process each key-value pair
  ITER=0
  for key in $keys
  do
    local value=${values_array[$ITER]}
    process_yaml_key "$key" "$value"
    ITER=$(expr $ITER + 1)
  done
}

# Function to execute the CLI with the provided arguments
execute_cli() {
  local app_name="$1"
  shift
  local cli_path="$app_name"

  # # Modify this command to run your actual CLI
  # # Currently, it just echoes the CLI path and arguments
  echo "Executing: $cli_path $@"
  $cli_path $@
}

# Usage: ./yaml_to_cli.sh <input_file.yaml>
if [[ $# -eq 0 ]]; then
  echo "Usage: $0 <input_file.yaml>"
  exit 1
fi

# Check if APP_NAME environment variable is set
if [[ -z "$APP_NAME" ]]; then
  echo "Error: APP_NAME environment variable is not set."
  exit 1
fi

# Check if CONFIG_NAME environment variable is set
if [[ -z "$CONFIG_NAME" ]]; then
  echo "Error: CONFIG_NAME environment variable is not set."
  exit 1
fi


# Parse the YAML file and convert keys to CLI arguments for the given app name
arguments=$(parse_yaml_file "$1" "$CONFIG_NAME")

# Execute the CLI with the parsed arguments
execute_cli "$APP_NAME" $arguments
