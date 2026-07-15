Output examples/demos/approval.gif
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

Type "ulx run approval.ulx RefundRequest --arg order_id=X123 --arg amount=42.50 --provider anthropic --run-id approval-demo --no-cache"
Enter
Sleep 8s

Type "ulx approve approval-demo --value 'approved, refund issued' --provider anthropic"
Enter
Sleep 3s
