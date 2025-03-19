import socket

client_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)

server_address = ('127.0.0.1', 43500)
client_socket.connect(server_address)

try:
    message = 'Hello, Server!'
    client_socket.sendall(message.encode('utf-8'))

    data = client_socket.recv(1024)
    print('Received:', data.decode('utf-8'))

finally:
    client_socket.close()
