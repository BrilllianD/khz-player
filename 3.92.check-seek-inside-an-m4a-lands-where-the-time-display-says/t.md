# Check: seek inside an M4A lands where the time display says

Fix A13 (defae47). Open an M4A/AAC file (there is none in ~/Music; take one from elsewhere or convert: ffmpeg -i x.flac -c:a aac x.m4a). Seek with the bar and with Left/Right. The audio heard must match the time display, with no jump of seconds or repeat. Not run by Claude: needs ears and no M4A was available.
