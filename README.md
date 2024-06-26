# HackerOne tracker
[![GitHub stars](https://img.shields.io/github/stars/hackermondev/hackerone-tracker)](https://github.com/hackermondev/hackerone-tracker/stargazers)
[![License](https://img.shields.io/github/license/hackermondev/hackerone-tracker)](LICENSE)

## Overview
Monitor HackerOne reports and track changes in the leaderboard of programs using a Discord webhook.

It uses the HackerOne GraphQL API to poll for new reports and leaderboard changes every 5 minutes, then sends a message to the webhook you conifugre.

[HackerOne reputation overview](https://docs.hackerone.com/hackers/reputation.html)
![showcase](https://i.imgur.com/g2J0xRK.png)


## Installation

Clone the repository (requires [Docker](https://docs.docker.com/get-docker/) and [Docker Compose](https://docs.docker.com/compose/install/)):
```bash
git clone https://github.com/hackermondev/hackerone-tracker
```
or clone on Replit:<br>
[![Run on Repl.it](https://replit.com/badge/github/hackermondev/hackerone-tracker)](https://replit.com/new/github/hackermondev/hackerone-tracker)



### Setup your configuration (config.example.yaml):
```yaml
discord:
  redis: redis://redis:6379 # Don't change this if you're using the default Docker compose/Replit configuration
  webhook_url: "" # Discord webhook URL (the format has to be: https://discord.com/api/webhooks/{webhook_id}/{webhook_token})

poller:
  redis: redis://redis:6379 # Don't change this if you're using the default Docker compose/Replit configuration
  handle: "" # HackerOne team handle
  session_token: "" # HackerOne session token (the "__Host-session" cookie), this is only required if you're tracking a private team
```
(If you're entering your session token and using Replit, make sure your repl is set to private. You'll also need to make sure you're logged in with HackerOne on the "2 weeks" session option and update your session token every 2 weeks in config)

If you're wish to track leaderboard changes and reports in all public programs, simply remove the `handle: ""` line.

After entering your config, **rename the file to ``config.yaml``**. If you're using Replit, simply click the ``Run`` button, otherwise with Docker compose run: ``sudo docker compose up --build -d``. Wait for it to build (this can take up to 5 minutes) and then you should now be tracking the leaderboad changes.

If you're using Replit, make sure to enable "Always On" with Replit to ensure it keeps running.


## Contributing
Pull requests are welcome. For major changes, please open an issue first to discuss what you would like to change.