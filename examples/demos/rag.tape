Output examples/demos/rag.gif
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

Type "ulx run rag.ulx Caption --arg photo=fixtures/sample.jpg --no-cache"
Enter
Sleep 8s

Type "ulx run rag.ulx AnsweredByRAG --arg question='What is the PTO policy?' --no-cache"
Enter
Sleep 8s
