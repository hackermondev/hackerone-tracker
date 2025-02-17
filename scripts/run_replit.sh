echo "                                  .__  __          
  ______ ____ ___  _____ _________|__|/  |_ ___.__.
 /  ___// __ \\  \/  /  |  \_  __ \  \   __<   |  |
 \___ \\  ___/ >    <|  |  /|  | \/  ||  |  \___  |
/____  >\___  >__/\_ \____/ |__|  |__||__|  / ____|
     \/     \/      \/                      \/     "


echo "Starting Redis server in the background..."
redis-server --save 60 1 --dbfilename dump.rdb --daemonize yes 

if [ ! -f "config.yaml" ]; then
  echo "Error: Could not fine config.yaml file. Make sure you rename config.example.yaml to config.yaml and fill in the values."
  exit 1
fi


if [ ! -d "target/release/sexurity-poller" ]; then
  echo "Building project..."
  cargo build --release
fi

sed -i -e 's/redis:6379/localhost:6379/g' config.yaml # Replace redis://redis:6379 to redis://localhost:6379 in config file
echo "Starting sexurity..."
RUST_LOG=info APP_NAME=./target/release/sexurity-poller CONFIG_NAME=poller bash scripts/yaml_to_cli.sh config.yaml &
RUST_LOG=info APP_NAME=./target/release/sexurity-discord CONFIG_NAME=discord bash scripts/yaml_to_cli.sh config.yaml
