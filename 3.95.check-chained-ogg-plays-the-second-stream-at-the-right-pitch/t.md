# Check: chained Ogg plays the second stream at the right pitch

A14 (622b742). Make two Ogg Vorbis files at different rates (ffmpeg -i x.flac -ar 44100 a.ogg; ffmpeg -i y.flac -ar 32000 b.ogg) and join them: cat a.ogg b.ogg > chained.ogg. Play it in rmp: the second part plays at the right pitch and does not stop early. With RUST_LOG=rmp=debug the log shows 'new stream, N Hz, C ch' at the boundary. The automated ignored test chained_ogg_real_file passed on ffmpeg-built chains; this check is by ear.
