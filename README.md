# OldMusa Server Rust
## A bit of backstory
This is an old project that I developed with CNR (Italy's National Research Institute).
Initially CNR contacted our high-school class to develop the initial project that you can still find [here](https://github.com/OldMusa-5H/OldMusaServer).
After we finished that project and we graduated they contacted me to continue working but offering payment, and I accepted.
During covid (after 5 months of work) they ghosted me and their promise for some retribution has been sweeped under the rug, they haven't responded since.
So just take it as a lesson, contracts are good and legally binding, use them folks.
Does this have documentation? No, and it never will, I don't want to waste even more time on this.

## About OldMusa
This is an app to monitor sensors in museums using CNR database.
The android frontend can be used to have useful data visualizations, navigate sensor maps and research
historic data. You can also setup ranges for each sensor and when the value goes off range (ex. a
temperature gets too high, or a place too moist) you will be alerted with a notification.

## About Project
The server is the counterpart of [OldMusa's Android app](https://gitlab.com/oldmusa/oldmusaapp), and a newer version of [An old python-based REST server](https://github.com/OldMusa-5H/OldMusaServer).
This manages users, notifications, permissions, sensor data sync and GraphQL communication.

## Code
This is my first rust project ever, I wasn't that experienced and rust was still young (futures compat amirite?).
Don't judge me on code quality, I'm better now, I promise.

