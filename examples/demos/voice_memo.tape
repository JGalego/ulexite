Output examples/demos/voice_memo.gif
Set Shell "bash"
Set FontSize 15
Set Width 1200
Set Height 650
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 40ms
Set Framerate 24

Type "cd examples"
Enter
Sleep 500ms

Type "ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav --no-cache"
Enter
Sleep 15s
