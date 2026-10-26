#include "pharovm/threadSafeQueue/threadSafeQueue.h"
#include "pharovm/semaphores/platformSemaphore.h"

#include <stdio.h>
#include <stdlib.h>

/**
 *  A queue implemented as a linked list with a link to the first and last nodes
 *  A mutex serves to mark a critical region and synchronize changes to the queue
 *  A semaphore serves to indicate if there are elements to take from the queue and block if there are none
 **/
struct __TSQueue {
	struct __TSQueueNode *first;
	struct __TSQueueNode *last;
	Semaphore *mutex;
	Semaphore *semaphore;
};

/**
 *  A node in the queue
 **/
typedef struct __TSQueueNode {
	void *element;
	struct __TSQueueNode *next;
} TSQueueNode;

/**
 *  Create a new queue in heap
 *  Returns a pointer to the newly created queue
 **/
TSQueue *threadsafe_queue_new(Semaphore *semaphore) {
	Semaphore* mutex;
	mutex = platform_semaphore_new(1);
	if (mutex == NULL) {
		perror("mutex initialization error in make_queue");
		return 0;
	}

	TSQueue *queue = (TSQueue *) malloc(sizeof(TSQueue));
	queue->mutex = mutex;
	queue->semaphore = semaphore;
	queue->first = NULL;
	queue->last = NULL;
	return queue;
}

/**
 *  Free a queue in heap and all its nodes
 *  Does not free the elements pointed by the nodes, as they are owned by the user
 *  Fails if pointer is invalid
 *  Nobody may use the queue any longer, so the mutex is freed as well
 **/
void threadsafe_queue_free(TSQueue *queue) {
	Semaphore *mutex = queue->mutex;
	platform_semaphore_wait(mutex);
	TSQueueNode *node = queue->first;
	TSQueueNode *next_node = node;
	while (node) {
		next_node = node->next;
		free(node);
		node = next_node;
	}
	free(queue);
	platform_semaphore_signal(mutex);
	mutex->free(mutex);
}

/**
 *  Return the number of elements in the queue
 **/
int threadsafe_queue_size(TSQueue *queue) {
    int size = 0;
    TSQueueNode *node;
	platform_semaphore_wait(queue->mutex);
    node = queue->first;
    while(node){
        size++;
        node = node->next;
    }
	platform_semaphore_signal(queue->mutex);
    return size;
}

/**
 *  Put an element at the end of in the thread safe queue
 *  Only one process may modify the queue at a single point in time
 *  Allocates a new node and puts the element into it
 **/
void threadsafe_queue_put(TSQueue *queue, void *element) {
	TSQueueNode *node = (TSQueueNode *) malloc(sizeof(TSQueueNode));
	node->element = element;
	node->next = NULL;

	platform_semaphore_wait(queue->mutex);
	if (!queue->first) {
		queue->first = node;
		queue->last = node;
	} else {
		queue->last->next = node;
		queue->last = node;
	}
	platform_semaphore_signal(queue->mutex);

	queue->semaphore->signal(queue->semaphore);
}

/**
 *  Take the first element from the thread safe queue
 *  Blocks the calling thread if there are none elements
 *
 *  Only one process may modify the queue at a single point in time
 *  Frees the node and returns the element stored in it
 **/
void *threadsafe_queue_take(TSQueue *queue) {
	//Block until the queue has elements
	if (queue->semaphore->wait(queue->semaphore) != 0){
		perror("Failed semaphore wait on thread safe queue");
		return NULL;
	}

	/*
	 * Read the first node under the mutex too: the semaphore of the callback queue is
	 * a Pharo semaphore whose wait does nothing, so only the mutex orders this read
	 * after the producer's writes to the node (and two consumers never take one node)
	 */
	platform_semaphore_wait(queue->mutex);
	TSQueueNode *node = queue->first;

	if(node == NULL) {
		platform_semaphore_signal(queue->mutex);
		return NULL;
	}

	queue->first = node->next;
	if (queue->first == NULL) {
		queue->last = NULL;
	}
	platform_semaphore_signal(queue->mutex);

	void *element = node->element;
	free(node);
	return element;
}
